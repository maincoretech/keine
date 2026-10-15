//! Static-analysis friction only: all decryption material still ships offline.

use std::fmt::Write;

pub fn generate(share_a: &[u8; 32], share_b: &[u8; 32], public_key: &[u8; 32]) -> String {
    let mut source = String::from(
        "#[inline(never)]\npub(super) fn load() -> ([u8; 32], [u8; 32]) {\n\
         use std::hint::black_box;\nlet mut key = [0u8; 32];\n",
    );
    // The publisher already supplies fresh random shares for each bundle.
    // Vary word order, masks, rotations and one of two reversible expressions;
    // do not add a second random generator or change the package's stable key.
    let mut order = [0, 1, 2, 3, 4, 5, 6, 7];
    order.sort_by_key(|&index| share_a[index]);
    for index in order {
        let start = index * 4;
        let mask = u32::from_le_bytes(share_a[start..start + 4].try_into().unwrap());
        let other = u32::from_le_bytes(share_b[start..start + 4].try_into().unwrap());
        let word = mask ^ other;
        let rotation = (mask >> 8) % 31 + 1;
        let (encoded, operation) = if mask & 1 == 0 {
            ((word ^ mask).rotate_right(rotation), "^")
        } else {
            (
                word.wrapping_sub(mask).rotate_right(rotation),
                ".wrapping_add",
            )
        };
        // black_box is a best-effort optimizer hint, not a security primitive.
        // Inspect actual release machine code after compiler/target changes.
        let value = format!("black_box(0x{encoded:08x}u32).rotate_left({rotation})");
        let mask = format!("black_box(0x{mask:08x}u32)");
        let expression = if operation == "^" {
            format!("{value} ^ {mask}")
        } else {
            format!("{value}{operation}({mask})")
        };
        writeln!(
            source,
            "key[{start}..{}].copy_from_slice(&({expression}).to_le_bytes());",
            start + 4,
        )
        .unwrap();
    }
    writeln!(source, "(key, {public_key:?})\n}}").unwrap();
    source
}
