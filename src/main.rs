#[macro_use]
extern crate rocket;

pub(crate) mod launch;
pub(crate) mod servers;
pub(crate) mod statuses;
pub(crate) mod versions;

fn main() {
	launch::main();
}
