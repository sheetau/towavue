Native shader static/header source materials (evaluation only)

This kit preserves the exact glslang, SPIRV-Tools, SPIRV-Headers and
Vulkan-Headers package inputs recorded by the audited shaderc/Vulkan Loader
builds, plus glslang's older SPIRV-Tools public-header input (five packages).
They are not additional runtime DLL owners. INPUTS.json records fixed
upstream locations, hashes, selected source documents and package provenance.
Full original sources, recipes and patches are retained without modification.
Package archives/signatures are checked inputs, not copied binary payloads.

The local audit verified all five package signatures with the existing MSYS2
keyring. The offline collector checks their recorded hashes, not signatures.
Source archive signatures, patches applied and native rebuilds are not claimed.

Keep the complete glslang original LICENSE.txt and generated parser notices.
The actual selected parser has the GNU Bison exception even though the root
license introduction says Bison was removed. Do not remove that exception or
flatten NVIDIA, inherited MIT/BSD or generated SPIR-V header notices.
SPIRV-Tools generators use SPIRV-Headers grammar data. Header source notices
also cover documentation/generator-only material, including jsoncpp. Vulkan's
MIT-only parse_dependency.py has inherited Paul McGuire attribution; do not
silently treat every file as the same Apache/MIT choice.

This is not a complete static/link dependency inventory or publication kit.
GCC/GCC-libs 16.1.0-5 remain separate follow-up work. glslang's build-time
SPIRV-Tools 350.1 public headers match the retained original sources; keep
Khronos/AMD, Google and Pierre Moreau attribution. Their includes reference
each other and standard C/C++ headers, not SPIRV-Headers grammar data.
Shaderc's own SPIRV-Tools input is 357.0. Do not infer a second old Tools
implementation at final link merely from glslang's build-time package list.
Source-only tools/tests/docs are not automatically runtime contributions.
Final per-component notices, source-access assembly and distribution approval
remain open. No application, installed package or runtime is changed.
