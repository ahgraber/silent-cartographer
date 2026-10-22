#[path = "../shared.rs"]
mod shared;

use shared::{SharedTr, SharedType};

fn main() {
    println!("{}", SharedType.shared());
}
