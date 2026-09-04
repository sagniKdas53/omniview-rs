fn main() {
    println!("cargo:rerun-if-changed=blender_xdnd.c");
    println!("cargo:rerun-if-changed=xdnd.h");
    println!("cargo:rerun-if-changed=src/c_xdnd.c");
    println!("cargo:rerun-if-changed=src/c_xdnd.h");

    cc::Build::new()
        .file("blender_xdnd.c")
        .file("src/c_xdnd.c")
        .include(".")
        .compile("xdnd_drag");

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=m");
}
