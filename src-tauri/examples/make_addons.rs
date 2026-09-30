//! The add-on list built into this version, as addons.json: cargo run --example make_addons > addons.json
//! (tools/release.ps1 publishes it, signed, with each release).

fn main() {
    print!("{}", paginaut_lib::addons::builtin_json());
}
