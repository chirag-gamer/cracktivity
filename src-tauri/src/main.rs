#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Role dispatch (spec §1): `--mcp` runs the rmcp stdio server with no GUI
    // init; stdout is protocol, logs go to stderr (D-012). GUI is the default.
    if std::env::args().any(|a| a == "--mcp") {
        cracktivity_lib::mcp::serve_stdio();
        return;
    }
    cracktivity_lib::run();
}
