use crate::prelude::*;
use append_only_vec::AppendOnlyVec;
use bevy_tasks::{ComputeTaskPool, TaskPool, ThreadExecutor};
use bones_lib::ecs::utils::*;
use parking_lot::Mutex;
use piccolo::{
    meta_ops::{self, MetaResult},
    AnyCallback, AnySequence, AnyUserData, CallbackReturn, Closure, Context, Error, Fuel, Lua,
    ProtoCompileError, Sequence, SequencePoll, Stack, StaticCallback, StaticClosure, StaticTable,
    Table, Thread, ThreadMode, Value,
};
use send_wrapper::SendWrapper;
use std::sync::Arc;

#[macro_use]
mod freeze;
use freeze::*;

mod asset;
pub use asset::*;

pub mod bindings;

/// Install the scripting plugin.
pub fn lua_game_plugin(game: &mut Game) {
    // Register asset type.
    LuaScript::schema();

    // Add `SchemaLuaMetatable` type data for common types.
    bindings::register_lua_typedata();

    // Initialize the lua engine resource.
    game.init_shared_resource::<LuaEngine>();
}

/// A frozen reference to the ECS [`World`].
///
// This type can be converted into lua userdata for accessing the world from lua.
#[derive(Deref, DerefMut, Clone)]
pub struct WorldRef(Frozen<Freeze![&'freeze World]>);

impl WorldRef {
    /// Convert this [`WorldRef`] into a Lua userdata.
    pub fn into_userdata<'gc>(
        self,
        ctx: Context<'gc>,
        world_metatable: Table<'gc>,
    ) -> AnyUserData<'gc> {
        let data = AnyUserData::new_static(&ctx, self);
        data.set_metatable(&ctx, Some(world_metatable));
        data
    }
}

/// Resource used to access the lua scripting engine.
#[derive(HasSchema, Clone)]
#[schema(no_default)]
pub struct LuaEngine {
    /// The thread-local task executor that is used to spawn any tasks that need access to the
    /// lua engine which can only be accessed on it's own thread.
    executor: Arc<ThreadExecutor<'static>>,
    /// The lua engine state container.
    state: Arc<SendWrapper<EngineState>>,
}

/// Internal state for [`LuaEngine`]
struct EngineState {
    /// The Lua engine.
    lua: Mutex<Lua>,
    /// Persisted lua data we need stored in Rust, such as the environment table, world
    /// metatable, etc.
    data: LuaData,
    /// Cache of the content IDs of loaded scripts, and their compiled lua closures.
    compiled_scripts: Mutex<HashMap<Cid, StaticClosure>>,
}

impl Default for EngineState {
    fn default() -> Self {
        // Initialize an empty lua engine and our lua data.
        Self {
            lua: Mutex::new(Lua::core()),
            data: default(),
            compiled_scripts: default(),
        }
    }
}

impl Default for LuaEngine {
    /// Initialize the Lua engine.
    fn default() -> Self {
        // Make sure the compute task pool is initialized
        ComputeTaskPool::init(TaskPool::new);

        #[cfg(not(target_arch = "wasm32"))]
        let executor = {
            let (send, recv) = async_channel::bounded(1);

            // Spawn the executor task that will be used for the lua engine.
            let pool = ComputeTaskPool::get();
            pool.spawn_local(async move {
                let executor = Arc::new(ThreadExecutor::new());
                send.try_send(executor.clone()).unwrap();

                let ticker = executor.ticker().unwrap();
                loop {
                    ticker.tick().await;
                }
            })
            .detach();
            pool.with_local_executor(|local| while local.try_tick() {});

            recv.try_recv().unwrap()
        };

        #[cfg(target_arch = "wasm32")]
        let executor = Arc::new(ThreadExecutor::new());

        LuaEngine {
            executor,
            state: Arc::new(SendWrapper::new(default())),
        }
    }
}

impl LuaEngine {
    /// Access the lua engine to run code on it.
    pub fn exec<'a, F: FnOnce(&mut Lua) + Send + 'a>(&self, f: F) {
        let pool = ComputeTaskPool::get();

        // Create a new scope spawned on the lua engine thread.
        pool.scope_with_executor(false, Some(&self.executor), |scope| {
            scope.spawn_on_external(async {
                f(&mut self.state.lua.lock());
            });
        });
    }

    /// Run a lua script as a system on the given world.
    pub fn run_script_system(&self, world: &World, script: Handle<LuaScript>) {
        self.exec(|lua| {
            Frozen::<Freeze![&'freeze World]>::in_scope(world, |world| {
                // Wrap world reference so that it can be converted to lua userdata.
                let world = WorldRef(world);

                lua.try_run(|ctx| {
                    // Create a thread
                    let thread = Thread::new(&ctx);

                    // Fetch the env table
                    let env = ctx
                        .state
                        .registry
                        .fetch(&self.state.data.table(ctx, bindings::env));

                    // Compile the script
                    let closure = world.with(|world| {
                        let asset_server = world.resource::<AssetServer>();
                        let cid = *asset_server
                            .store
                            .asset_ids
                            .get(&script.untyped())
                            .expect("Script asset not loaded");

                        let mut compiled_scripts = self.state.compiled_scripts.lock();
                        let closure = compiled_scripts.get(&cid);

                        Ok::<_, ProtoCompileError>(match closure {
                            Some(closure) => ctx.state.registry.fetch(closure),
                            None => {
                                let asset = asset_server.store.assets.get(&cid).unwrap();
                                let source = &asset.data.cast_ref::<LuaScript>().source;
                                let closure = Closure::load_with_env(ctx, source.as_bytes(), env)?;
                                compiled_scripts
                                    .insert(cid, ctx.state.registry.stash(&ctx, closure));

                                closure
                            }
                        })
                    })?;

                    // Insert the world ref into the global scope
                    let world = world.into_userdata(
                        ctx,
                        ctx.state
                            .registry
                            .fetch(&self.state.data.table(ctx, bindings::world_metatable)),
                    );
                    env.set(ctx, "world", world)?;

                    // Start the thread
                    thread.start(ctx, closure.into(), ())?;

                    // Run the thread to completion
                    let mut fuel = Fuel::with_fuel(i32::MAX);
                    loop {
                        // If the thread is ready
                        if matches!(thread.mode(), ThreadMode::Normal) {
                            // Step it
                            thread.step(ctx, &mut fuel)?;
                        } else {
                            break;
                        }

                        // Handle fuel interruptions
                        if fuel.is_interrupted() {
                            break;
                        }
                    }

                    // Take the thread result and print any errors
                    let result = thread.take_return::<()>(ctx)?;
                    if let Err(e) = result {
                        tracing::error!("{e}");
                    }

                    Ok(())
                })
                .unwrap();
            });
        });
    }
}

/// Static lua tables and callbacks
pub struct LuaData {
    callbacks: AppendOnlyVec<(fn(&LuaData, Context) -> StaticCallback, StaticCallback)>,
    tables: AppendOnlyVec<(fn(&LuaData, Context) -> StaticTable, StaticTable)>,
}
impl Default for LuaData {
    fn default() -> Self {
        Self {
            callbacks: AppendOnlyVec::new(),
            tables: AppendOnlyVec::new(),
        }
    }
}

impl LuaData {
    /// Get a table from the store, initializing it if necessary.
    pub fn table(&self, ctx: Context, f: fn(&LuaData, Context) -> StaticTable) -> StaticTable {
        for (other_f, table) in self.tables.iter() {
            if *other_f == f {
                return table.clone();
            }
        }
        let new_table = f(self, ctx);
        self.tables.push((f, new_table.clone()));
        new_table
    }

    /// Get a callback from the store, initializing if necessary.
    pub fn callback(
        &self,
        ctx: Context,
        f: fn(&LuaData, Context) -> StaticCallback,
    ) -> StaticCallback {
        for (other_f, callback) in self.callbacks.iter() {
            if *other_f == f {
                return callback.clone();
            }
        }
        let new_callback = f(self, ctx);
        self.callbacks.push((f, new_callback.clone()));
        new_callback
    }
}

/// Schema [type data][TypeDatas] that may be used to create a custom lua metatable for this type
/// when it is accessed in Lua scripts
#[derive(HasSchema, Clone, Copy, Debug)]
#[schema(no_default)]
struct SchemaLuaMetatable(pub fn(&LuaData, Context) -> StaticTable);

/// A reference to a resource
struct ResourceRef {
    cell: UntypedAtomicResource,
    path: Ustr,
}
