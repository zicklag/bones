use bones_reactive::*;

fn main() {
    let (count, set_count) = create_signal(12);

    create_effect(|_| {
        println!("The count is: {}", count.get());
    });
}
