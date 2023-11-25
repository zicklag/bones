//! General utilities for [Bones] meta-engine crates.
//!
//! [Bones]: https://fishfolk.org/development/bones/introduction/
//!

use std::{cell::RefCell, marker::PhantomData, thread::ThreadId};

use bones_schema::prelude::{bones_utils::SmallVec, *};
use petgraph::{
    graph::{Graph, NodeIndex},
    Direction::*,
};

thread_local! {
    /// The global thread-local runtime.
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default())
}

fn with_runtime<F: FnOnce(&Runtime) -> R, R>(f: F) -> R {
    RUNTIME.with_borrow(|runtime| f(runtime))
}

#[derive(Default, Debug, Clone)]
pub struct Runtime {
    graph: RefCell<Graph<Node, ()>>,
    current_effect_deps: RefCell<Option<Vec<NodeId>>>,
}

use node_id::NodeId;
mod node_id {
    use super::*;

    /// A reactive node identifier.
    #[derive(Clone, Copy, Debug)]
    pub struct NodeId {
        idx: NodeIndex,
        thread: ThreadId,
    }
    impl NodeId {
        /// Create a new node with the given index.
        pub fn new(idx: NodeIndex) -> NodeId {
            Self {
                idx,
                thread: std::thread::current().id(),
            }
        }

        /// Get the thread that this node was created on.
        pub fn thread(&self) -> ThreadId {
            self.thread
        }

        /// # Panics
        /// Panics if the node was created for a different thread.
        pub fn idx(&self) -> NodeIndex {
            self.ensure_local();
            self.idx
        }

        /// # Panics
        /// Panics if the node was created for a different thread.
        pub fn ensure_local(&self) {
            assert_eq!(
                self.thread,
                std::thread::current().id(),
                "Attempted to use a signal on a different thread than the one it was created on."
            );
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Node {
    state: NodeState,
    value: SchemaBox,
}

#[derive(Default, Debug, Clone)]
pub enum NodeState {
    Clean,
    Check,
    #[default]
    Dirty,
}

pub struct ReadSignal<T> {
    id: NodeId,
    _phantom: PhantomData<T>,
}
impl<T> ReadSignal<T> {
    fn from_id(id: NodeId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }
}
pub struct WriteSignal<T> {
    id: NodeId,
    _phantom: PhantomData<T>,
}
impl<T> WriteSignal<T> {
    fn from_id(id: NodeId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }
}
pub struct RwSignal<T> {
    id: NodeId,
    _phantom: PhantomData<T>,
}
impl<T> RwSignal<T> {
    fn from_id(id: NodeId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }
}

pub trait SignalWrite<T> {
    fn set(&self, value: T);
    fn update<F: FnOnce(&mut T) -> R, R>(&self, f: F) -> R;
}
pub trait SignalRead<T>: SignalReadRef<T> {
    fn get(&self) -> T;
}
pub trait SignalReadRef<T> {
    fn with<F: FnOnce(&T) -> R, R>(&self, f: F) -> R;
}
impl<T: Clone + HasSchema> SignalRead<T> for ReadSignal<T> {
    fn get(&self) -> T {
        RwSignal::from_id(self.id).get()
    }
}
impl<T: HasSchema> SignalReadRef<T> for ReadSignal<T> {
    fn with<F: FnOnce(&T) -> R, R>(&self, f: F) -> R {
        RwSignal::from_id(self.id).with(f)
    }
}
impl<T: Clone + HasSchema> SignalRead<T> for RwSignal<T> {
    fn get(&self) -> T {
        with_runtime(|runtime| {
            let node = &runtime.graph.borrow()[self.id.idx()];
            if let Some(deps) = &mut *runtime.current_effect_deps.borrow_mut() {
                deps.push(self.id);
            }
            node.value.cast_ref::<T>().clone()
        })
    }
}
impl<T: HasSchema> SignalReadRef<T> for RwSignal<T> {
    fn with<F: FnOnce(&T) -> R, R>(&self, f: F) -> R {
        with_runtime(|runtime| {
            let node = &runtime.graph.borrow()[self.id.idx()];
            let value = node.value.cast_ref::<T>();
            f(value)
        })
    }
}
impl<T: HasSchema> SignalWrite<T> for RwSignal<T> {
    fn set(&self, value: T) {
        self.update(|old_value| *old_value = value);
    }
    fn update<F: FnOnce(&mut T) -> R, R>(&self, f: F) -> R {
        with_runtime(|runtime| {
            let mut graph = runtime.graph.borrow_mut();
            let node_idx = self.id.idx();
            let node = &mut graph[node_idx];
            let value = node.value.cast_mut::<T>();
            let r = f(value);
            node.state = NodeState::Dirty;

            fn traverse_update<const S: usize, const E: usize>(
                graph: &mut Graph<Node, ()>,
                neighbor_stack: &mut SmallVec<[NodeIndex; S]>,
                effects_to_run: &mut SmallVec<[NodeIndex; E]>,
                nodeidx: NodeIndex,
            ) {
                let mut neighbor_count = 0;
                for neighbor in graph.neighbors_directed(nodeidx, Incoming) {
                    neighbor_count += 1;
                    neighbor_stack.push(neighbor);
                }

                // This is an effect
                if neighbor_count == 0 {
                    effects_to_run.push(nodeidx);

                // This is a signal
                } else {
                    for _ in 0..neighbor_count {
                        let idx = neighbor_stack.pop().unwrap();
                        graph[idx].state = NodeState::Check;
                        traverse_update(graph, neighbor_stack, effects_to_run, idx);
                    }
                }
            }

            let mut effects_to_run = SmallVec::<[NodeIndex; 16]>::new();
            let mut neighbor_stack = SmallVec::<[NodeIndex; 64]>::new();
            traverse_update(
                &mut graph,
                &mut neighbor_stack,
                &mut effects_to_run,
                node_idx,
            );

            for efect in effects_to_run {
                todo!("Run the effect");
            }

            r
        })
    }
}

pub fn create_signal<T: HasSchema>(value: T) -> (ReadSignal<T>, WriteSignal<T>) {
    with_runtime(|runtime| {
        let idx = runtime.graph.borrow_mut().add_node(Node {
            state: NodeState::Dirty,
            value: SchemaBox::new(value),
        });
        let node = NodeId::new(idx);

        (ReadSignal::from_id(node), WriteSignal::from_id(node))
    })
}

pub struct Effect<R> {
    id: NodeId,
    _phantom: PhantomData<R>,
}
impl<R> Effect<R> {
    pub fn from_id(id: NodeId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }
}

pub fn create_effect<F: FnMut(Option<R>) -> R, R: HasSchema>(mut f: F) -> Effect<R> {
    with_runtime(|runtime| {
        // Create dependency list
        {
            let deps_list = Vec::new();
            let mut deps = runtime.current_effect_deps.borrow_mut();
            if deps.is_some() {
                panic!("You cannot create an effect while inside of an effect.");
            }
            *deps = Some(deps_list);
        }

        // Run the effect once
        let r = f(None);

        // Create the node
        let node = Node {
            state: NodeState::Clean,
            value: SchemaBox::new(r),
        };
        // Insert the node
        let mut graph = runtime.graph.borrow_mut();
        let idx = graph.add_node(node);

        // Add dependencies as graph edges
        for dep in runtime.current_effect_deps.borrow_mut().take().unwrap() {
            graph.add_edge(idx, dep.idx(), ());
        }

        // Return the effect
        Effect::from_id(NodeId::new(idx))
    })
}
