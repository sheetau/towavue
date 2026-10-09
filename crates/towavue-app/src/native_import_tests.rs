//! Inspect the actual linked test host, not just the requested linker arguments.
use std::collections::BTreeSet;

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("PE u16"))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("PE u32"))
}

#[test]
fn linked_host_delays_codecs_and_keeps_diagnostics_eager() {
    let bytes = std::fs::read(std::env::current_exe().expect("test executable"))
        .expect("read linked test executable");
    assert_eq!(&bytes[..2], b"MZ");
    let pe = u32_at(&bytes, 0x3c) as usize;
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0");
    assert_eq!(u16_at(&bytes, pe + 4), 0x8664);
    let optional = pe + 24;
    assert_eq!(u16_at(&bytes, optional), 0x20b);
    let section_start = optional + u16_at(&bytes, pe + 20) as usize;
    let section_count = u16_at(&bytes, pe + 6) as usize;
    let offset = |rva: u32| {
        (0..section_count)
            .find_map(|i| {
                let section = section_start + i * 40;
                let base = u32_at(&bytes, section + 12);
                let raw_size = u32_at(&bytes, section + 16);
                rva.checked_sub(base)
                    .filter(|relative| *relative < raw_size)
                    .map(|relative| (u32_at(&bytes, section + 20) + relative) as usize)
            })
            .expect("file-backed PE directory RVA")
    };
    let read_names = |directory: usize, stride: usize, name_field: usize| {
        let rva = u32_at(&bytes, optional + 112 + directory * 8);
        let size = u32_at(&bytes, optional + 116 + directory * 8) as usize;
        assert_ne!(rva, 0, "missing PE import directory {directory}");
        let start = offset(rva);
        let mut names = BTreeSet::new();
        let mut terminated = false;
        for record in bytes[start..start + size].chunks_exact(stride) {
            if record.iter().all(|byte| *byte == 0) {
                terminated = true;
                break;
            }
            if directory == 13 {
                assert_eq!(u32_at(record, 0), 1, "delay-import RVAs required");
            }
            let name = &bytes[offset(u32_at(record, name_field))..];
            let end = name
                .iter()
                .position(|byte| *byte == 0)
                .expect("DLL terminator");
            names.insert(
                std::str::from_utf8(&name[..end])
                    .expect("DLL name")
                    .to_ascii_lowercase(),
            );
        }
        assert!(terminated, "unterminated PE import directory");
        names
    };
    let eager = read_names(1, 20, 12);
    let delayed = read_names(13, 32, 4);
    let expected: BTreeSet<_> = env!("TOWAVUE_DELAYED_FFMPEG_DLLS")
        .split(';')
        .map(str::to_owned)
        .collect();
    assert_eq!(expected.len(), 5);
    assert_eq!(delayed, expected);
    assert!(eager.is_disjoint(&delayed));
    assert!(
        eager
            .iter()
            .any(|name| name.starts_with("avutil-") && name.ends_with(".dll"))
    );
}
