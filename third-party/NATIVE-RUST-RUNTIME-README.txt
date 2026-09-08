Native Rust runtime source/notice materials (2026-09-08)

This is a preparation/audit bundle, not an approved release, linked SBOM or
complete compiler rebuild kit. Keep it with the native Rust crate materials
and native library source supplements. It applies to the MSYS2 Rust packages
recorded for rav1e (1.87.0-2) and libdovi (1.97.0-1), not the excluded BtbN
binary or towavue's own MSVC toolchain.

Four original Rust/rust-src package signatures were verified against the
existing MSYS2 keyring. Each package's .BUILDINFO matches the pinned PKGBUILD.
The collector checks fixed bytes offline, not signatures afresh. It does
not download, execute Cargo/build scripts, install a compiler, or copy the
compiler binary packages to its output. Use a fresh output directory.

The two package COPYRIGHT-library.html files are identical and their
out-of-tree dependency sections are empty. They are retained unchanged as
package evidence, not presented as complete standard-library notices.
COPYRIGHT.html is compiler-scope evidence. The licenses directory is the
package's text dictionary, not a declaration that every license applies to
the Windows DLLs. Source-package root and nested originals are also retained.

The full rust-src source packages and selected documents are included.
The source releases used to build the compilers are identified by recipe
hash but are not included; rust-src is not the whole compiler source.
All eleven recipe patch/configuration inputs and both recipes are preserved.
The recipes disable self-contained GNU link inputs; 1.97 also enables native
TLS. These are MSYS2 builds, not byte-equivalent official Rust distributions.
Non-Rust compiler/CRT/static link inputs still require separate scope checks.

Rust 1.87's original library lock identifies 42 registry archives; all match
its checksums and are included intact with selected original documents.
This includes other targets, tests and build dependencies. It does not mean
all 42 are linked on Windows. Rust 1.97's source package already contains its
vendored library dependencies, including compiler-builtins and libm sources.
Neither collection reconstructs historical Cargo host/target unit selection.

compiler_builtins 0.1.152 omits standalone license files from its crate.
Its VCS metadata points to compiler-builtins commit 52d96c47681ef504a8ad7398efffe53214898aab.
The original root LICENSE.txt and the license at its recorded libm submodule
commit 69219c491ee9f05761d2068fd6d4c7c0de6faa3a are preserved separately.
Keep individual math source headers as well; do not replace compound license
expressions with a root MIT-only label. The full crate retains those sources.

Fortanix SGX and r-efi/r-efi-alloc archives also omit standalone license files.
Their original sources/metadata remain intact, but their separate notice
resolution is not claimed complete. Do not assign these other-target
dependencies to the Windows DLLs merely because they occur in the lock.
Individual inline/generated-data attribution and actual GNU selection remain
separate work, as do patent/trademark, final-runtime and installer gates.
