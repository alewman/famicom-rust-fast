// SPDX-License-Identifier: MIT
//! Every mapper number the reference builds (famicom-python's `MAPPERS` at
//! famicom-rust-pin-1, and 559 from `e3e1474`) builds here too, from a synthetic
//! image. `e3e1474` also builds 99, the Vs. System's sockets, not carried yet
//! (README, "The repin of 2026-11-23").

use famicom_rust::cartridge::{build, BoardInfo, RomImage};

/// The reference's `sorted(MAPPERS)`.
const REFERENCE: &[u16] = &[
    0, 1, 2, 3, 4, 5, 7, 9, 10, 11, 12, 13, 14, 15, 16, 18, 19, 21, 22, 23, 24, 25, 26, 27, 28, 29,
    30, 31, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43, 45, 46, 47, 48, 50, 52, 56, 57, 58, 59, 60,
    61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 85,
    86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 103, 104, 105, 106, 107, 108, 111, 112, 113,
    114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 125, 126, 132, 133, 134, 136, 137, 138, 139,
    140, 141, 142, 143, 144, 145, 147, 148, 149, 150, 152, 153, 154, 155, 156, 157, 158, 159, 162,
    163, 164, 165, 167, 168, 170, 172, 173, 176, 177, 178, 180, 182, 183, 184, 185, 187, 188, 189,
    190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 205, 206, 207, 208, 209, 210, 211, 212, 213,
    215, 216, 218, 219, 221, 222, 224, 225, 226, 227, 228, 229, 232, 234, 235, 238, 240, 241, 242,
    243, 244, 245, 246, 249, 250, 252, 253, 254, 255, 258, 260, 262, 263, 264, 266, 268, 272, 284,
    285, 286, 289, 290, 292, 298, 299, 301, 302, 303, 304, 305, 306, 307, 308, 309, 312, 313, 314,
    319, 326, 328, 330, 332, 334, 346, 347, 351, 355, 359, 368, 374, 380, 389, 394, 396, 399, 400,
    406, 409, 412, 413, 415, 417, 418, 422, 446, 451, 470, 471, 487, 493, 512, 513, 515, 516, 517,
    518, 519, 521, 522, 523, 525, 526, 529, 530, 533, 534, 535, 538, 539, 540, 541, 544, 547, 548,
    549, 551, 552, 553, 554, 555, 557, 558, 559, 594,
];

#[test]
fn every_reference_mapper_builds() {
    let mut missing = Vec::new();
    for &mapper in REFERENCE {
        let image = RomImage {
            header: vec![0x4E, 0x45, 0x53, 0x1A, 32, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            trainer: Vec::new(),
            prg: vec![0; 512 * 1024],
            chr: vec![0; 256 * 1024],
            misc: Vec::new(),
            board: BoardInfo {
                prg_ram: 0x2000,
                ..BoardInfo::new(mapper)
            },
            nes2: false,
        };
        if let Err(error) = build(image, 12) {
            missing.push(format!("{mapper}: {error}"));
        }
    }
    assert!(missing.is_empty(), "not built: {missing:?}");
    assert_eq!(REFERENCE.len(), 295); // the reference's 296 less 99
}
