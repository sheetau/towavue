Native shader/API-header source materials (evaluation only)

This kit preserves the exact glslang, SPIRV-Tools, SPIRV-Headers and
Vulkan-Headers package inputs recorded by the audited shaderc/Vulkan Loader
builds, plus glslang's older SPIRV-Tools public-header input. Four additional
libplacebo inputs preserve fast_float, xxHash, glad and its older Vulkan
headers. The OpenCL ICD Loader's exact external OpenCL Headers package is
also retained (ten packages in total).
They are not additional runtime DLL owners. INPUTS.json records fixed
upstream locations, hashes, selected source documents and package provenance.
Full original sources, recipes and patches are retained without modification.
Package archives/signatures are checked inputs, not copied binary payloads.

OpenCL Headers 2~2026.05.29-1 is bound to the original OpenCL ICD Loader
build record. All eighteen installed headers match their original source
bytes; both sets are preserved with Apache-2.0 and actual Khronos copyrights.
The root license appendix is an example, not an author list. No standalone
NOTICE is present in the inspected source. Headers are API/build inputs,
not a vendor driver implementation or a new runtime DLL owner. The package
recipe installs the existing headers; this kit does not rebuild the loader
or reproduce its separate dispatch-generation environment.

The local audit verified all ten package signatures with the existing MSYS2
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

libplacebo uses inline xxHash, not a new xxHash runtime DLL. Preserve its
header's 2012-2023 attribution as well as the root LICENSE. fast_float's
MIT/Apache/Boost alternatives and Google Wuffs credit remain unchanged;
it is unrelated to Little CMS's GPL fast_float plugin. glad's generator
is MIT, while generated code has a distinct WTFPL/CC0 and Apache notice.
Keep the original GL/EGL registry data, Khronos platform headers and C
templates. eglplatform.h's actual Apache notice differs from the root
license introduction; do not flatten these into one package label.
libplacebo's Vulkan-Headers 350.1 is separate from Loader's 357.0 input.
These are source/build-record observations, not a historical regenerated
header or bit-identical library rebuild. Full original archives retain
source-only CLI/tests and their separate terms.

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
