// std::str::trim() is not const yet
pub const fn const_str_trim(s: &str) -> &str {
    let len = s.len();

    let bytes = s.as_bytes();
    let mut i = 0;
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let mut j = len;
    while j > i && bytes[j - 1].is_ascii_whitespace() {
        j -= 1;
    }

    unsafe {
        let ptr = s.as_ptr().add(i);
        let len = j - i;
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    }
}

// the nice thing about theses tests, is that it doesn't even compile if the
// implementation is incorrect, so the LSP tells me immediately
const _: () = const {
    if !const_str_trim("").is_empty() {
        panic!("implementation error");
    }
    if !const_str_trim(" ").is_empty() {
        panic!("implementation error");
    }
    if !const_str_trim("  ").is_empty() {
        panic!("implementation error");
    }
    if !const_str_trim("     \n  ").is_empty() {
        panic!("implementation error");
    }

    if !const_str_eq(const_str_trim(" a"), "a") {
        panic!("implementation error");
    }

    if !const_str_eq(const_str_trim(" abc"), "abc") {
        panic!("implementation error");
    }

    if !const_str_eq(const_str_trim("abc "), "abc") {
        panic!("implementation error");
    }

    if !const_str_eq(const_str_trim("   abc \n "), "abc") {
        panic!("implementation error");
    }
};

// PartialEq is not const, either
const fn const_str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let a = a.as_bytes();
    let b = b.as_bytes();
    let len = a.len();
    let mut i = 0;
    while i < len {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
