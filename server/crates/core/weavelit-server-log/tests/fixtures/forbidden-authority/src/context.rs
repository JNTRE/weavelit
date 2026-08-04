use std::path::PathBuf;

use weavelit_server_log::TrustedLogModuleContext;

fn main() {
    let root = PathBuf::from("/srv/weavelit");
    let _context = TrustedLogModuleContext::new(root, [2; 16]);
}