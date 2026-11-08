//! Every `.acre` file in `examples/` parses, builds and draws.

use std::fs;

#[test]
fn every_example_draws() {
    let mut count = 0;
    for entry in fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "acre") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let program = acre::lang::parse(&src)
            .unwrap_or_else(|e| panic!("{}: {:?}: {}", path.display(), e.line_col(&src), e));
        let scene = acre::scene::build(&program)
            .unwrap_or_else(|e| panic!("{}: {:?}: {}", path.display(), e.line_col(&src), e));
        acre::render::draw_frame(&scene, 0.0);
        count += 1;
    }
    assert!(count > 0);
}
