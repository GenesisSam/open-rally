use std::path::Path;

pub const OCTET_STREAM: &str = "application/octet-stream";

/// 경로 구분자와 `..` 를 제거한다. 비면 `file`.
pub fn sanitize_filename(raw: &str) -> String {
    let mut s: String = raw
        .chars()
        .filter(|c| *c != '/' && *c != '\\' && !c.is_control())
        .collect();
    while s.contains("..") {
        s = s.replace("..", "");
    }
    let s = s.trim().to_string();
    if s.is_empty() || s.chars().all(|c| c == '.') {
        "file".to_string()
    } else {
        s
    }
}

pub fn kind_from_mime(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m == "image/svg+xml" {
        "file"
    } else if m.starts_with("image/") {
        "image"
    } else if m.starts_with("video/") {
        "video"
    } else {
        "file"
    }
}

fn is_inline(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    if m == "image/svg+xml" || m == "text/html" || m == "application/xhtml+xml" {
        return false;
    }
    m.starts_with("image/") || m.starts_with("video/")
}

/// RFC 3986 unreserved 문자만 남기고 나머지는 %XX 로 바꾼다 (UTF-8 바이트 단위).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn disposition(mime: &str, filename: &str) -> String {
    if is_inline(mime) {
        return "inline".to_string();
    }
    let clean: String = filename
        .chars()
        .filter(|c| *c != '"' && !c.is_control())
        .collect();
    if clean.is_ascii() {
        return format!("attachment; filename=\"{clean}\"");
    }
    // 비 ASCII 파일명: ASCII 폴백 + RFC 5987 filename* (Safari 가 raw UTF-8 을 깨뜨릴 수 있다)
    let ascii: String = clean.chars().filter(|c| c.is_ascii()).collect();
    let (stem, ext) = match ascii.rfind('.') {
        Some(i) if i > 0 => (&ascii[..i], &ascii[i..]),
        _ => (ascii.as_str(), ""),
    };
    let fallback = if stem.trim().is_empty() { format!("file{ext}") } else { ascii.clone() };
    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
        percent_encode(&clean)
    )
}

/// `dir` 안에 같은 이름이 있으면 `name-2.ext`, `name-3.ext` ... 를 돌려준다.
pub fn unique_in_dir(dir: &Path, filename: &str) -> String {
    if !dir.join(filename).exists() {
        return filename.to_string();
    }
    let (stem, ext) = match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    };
    let mut n = 2;
    loop {
        let candidate = format!("{stem}-{n}{ext}");
        if !dir.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

pub fn guess_mime(browser_mime: Option<&str>, filename: &str) -> String {
    match browser_mime {
        Some(m) if !m.trim().is_empty() && m != OCTET_STREAM => m.to_string(),
        _ => mime_guess::from_path(filename)
            .first_raw()
            .unwrap_or(OCTET_STREAM)
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_parts_and_keeps_unicode() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_filename("a/b\\c.txt"), "abc.txt");
        assert_eq!(sanitize_filename(""), "file");
        assert_eq!(sanitize_filename("..."), "file");
        assert_eq!(sanitize_filename("배포 도구 v2.dmg"), "배포 도구 v2.dmg");
    }

    #[test]
    fn kind_and_disposition_rules() {
        assert_eq!(kind_from_mime("image/png"), "image");
        assert_eq!(kind_from_mime("image/svg+xml"), "file");
        assert_eq!(kind_from_mime("video/mp4"), "video");
        assert_eq!(kind_from_mime("application/x-mach-binary"), "file");
        assert_eq!(disposition("image/png", "a.png"), "inline");
        assert_eq!(disposition("video/quicktime", "a.mov"), "inline");
        assert_eq!(
            disposition("image/svg+xml", "a.svg"),
            "attachment; filename=\"a.svg\""
        );
        assert_eq!(
            disposition("text/html", "a.html"),
            "attachment; filename=\"a.html\""
        );
        assert_eq!(
            disposition("application/octet-stream", "deploy-tool"),
            "attachment; filename=\"deploy-tool\""
        );
        assert_eq!(
            disposition("application/octet-stream", "a\"b\n.txt"),
            "attachment; filename=\"ab.txt\""
        );
    }

    #[test]
    fn disposition_encodes_non_ascii_filenames() {
        assert_eq!(
            disposition("application/octet-stream", "배포 도구.dmg"),
            "attachment; filename=\"file.dmg\"; filename*=UTF-8''%EB%B0%B0%ED%8F%AC%20%EB%8F%84%EA%B5%AC.dmg"
        );
        assert_eq!(
            disposition("text/plain", "한글 memo.txt"),
            "attachment; filename=\" memo.txt\"; filename*=UTF-8''%ED%95%9C%EA%B8%80%20memo.txt"
        );
        assert_eq!(percent_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(percent_encode("#1 x?.txt"), "%231%20x%3F.txt");
    }

    #[test]
    fn unique_in_dir_appends_counter() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "").unwrap();
        std::fs::write(d.path().join("a-2.txt"), "").unwrap();
        assert_eq!(unique_in_dir(d.path(), "a.txt"), "a-3.txt");
        assert_eq!(unique_in_dir(d.path(), "b.txt"), "b.txt");
        std::fs::write(d.path().join("noext"), "").unwrap();
        assert_eq!(unique_in_dir(d.path(), "noext"), "noext-2");
    }

    #[test]
    fn guess_mime_falls_back_to_extension() {
        assert_eq!(guess_mime(Some("image/png"), "x.bin"), "image/png");
        assert_eq!(guess_mime(None, "x.png"), "image/png");
        assert_eq!(guess_mime(Some(""), "x.mp4"), "video/mp4");
        assert_eq!(
            guess_mime(Some("application/octet-stream"), "x.jpg"),
            "image/jpeg"
        );
        assert_eq!(guess_mime(None, "deploy-tool"), "application/octet-stream");
    }
}
