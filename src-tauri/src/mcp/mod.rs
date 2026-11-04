//! MCP role (D-012): `cracktivity.exe --mcp` runs an rmcp stdio server with
//! no GUI. Tools land in Phase 4; the role must already stay quiet on stdout
//! (stdout is protocol) so a mis-spawned server can never corrupt a stream.

pub fn serve_stdio() {
    // Phase 4: rmcp serve over stdio. Until then the role exits cleanly;
    // stderr-only output keeps the protocol stream pristine (spec §9).
    eprintln!("cracktivity: mcp role not yet implemented");
}
