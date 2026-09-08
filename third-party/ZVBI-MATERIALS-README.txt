ZVBI source and provenance audit materials
========================================

This directory is evidence for an unresolved distribution concern. It is
not an approved LGPL runtime package, an installer payload, or permission
to redistribute the towavue candidate under its intended license terms.
No executable or DLL is copied into these materials.

The package, actual runtime DLL, original PKGBUILD, source archive and
package patch are byte-pinned in INPUTS.json. The package's .BUILDINFO
identifies the exact recipe. Its VCS source checksum is the SHA256 of an
uncompressed git archive of tag v0.2.45, not a codeload tar.gz checksum.

The annotated tag resolves to commit
d3a5ee9f2b047bf16cd1ee5ccf6ec05ee75409d0. Generate the archive with:

  git -C path/to/zvbi -c core.autocrlf=false -c core.abbrev=no archive \
      --format tar --output path/to/zvbi-0.2.45-git.tar v0.2.45

The result must have 4239360 bytes and SHA256
3dc234d716d1c51d53ae9b6b027775b3a6c9ced974bc1b246d749b44cca7d964.
Git for Windows with inherited core.autocrlf=true produces different
archive bytes; those bytes must not be accepted by changing the pin.
This does not claim local cryptographic verification of the tag signature.

The source archive has 224 regular files and 11 directories, with no
symlinks or unsafe paths. Preserve the full original archive and the
371-byte no-undefined patch. The generator extracts only selected original
license/scope files, does not apply the patch and does not build anything.
The patch passes git apply --check and changes only a linker option.

COPYING.md matches the notice in the binary package. The 2008 NEWS entry
describes the LGPL transition. However, pdc.c and packet-830.c were added
to the official archive in February 2009 with GPL headers and then added
to library sources. The current files and COPYING retain those notices.
The actual package DLL exposes corresponding local-code exports. The
GPL exp-vtx.c body, by contrast, is disabled with #if 0; its presence in
the source archive must not be confused with compiled implementation.

FFmpeg's teletext decoder registers VBI_EVENT_TTX_PAGE. The two packet.c
calls into packet-830.c are guarded by VBI_EVENT_LOCAL_TIME and
VBI_EVENT_PROG_ID. This helps identify a possible investigation boundary,
but an unused call path does not remove GPL-marked code from the shipped
DLL, establish a license grant, or prove an alternative build equivalent.
No stubs, codec removal, license-header edits or permission request are
implemented by this material preparation step.

See docs/NATIVE_RUNTIME_AUDIT.md for source-history and runtime evidence.
Permission scope or a verified feature-preserving alternative remains
necessary before distribution approval, along with the wider dependency,
installer, target-Windows and final-candidate quality gates.
