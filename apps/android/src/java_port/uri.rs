//! `java.net.URI` (OpenJDK `URI.Parser`), only the parts the server-address
//! helpers read: scheme, raw authority/user info/host/port/path/query/fragment.

/// The components of a parsed `java.net.URI` (`null` → `None`).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Uri {
    pub scheme: Option<String>,
    pub authority: Option<String>,
    pub user_info: Option<String>,
    pub host: Option<String>,
    /// `getPort()`: -1 when undefined.
    pub port: i32,
    pub path: Option<String>,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

/// `URISyntaxException` (the message is not used by the callers).
#[derive(Debug)]
pub struct SyntaxError;
type R<T> = Result<T, SyntaxError>;

fn is_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}
fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}
fn is_alphanum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}
fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}
fn is_mark(c: char) -> bool {
    "-_.!~*'()".contains(c)
}
fn is_unreserved(c: char) -> bool {
    is_alphanum(c) || is_mark(c)
}
fn is_reserved(c: char) -> bool {
    ";/?:@&=+$,[]".contains(c)
}
fn is_uric(c: char) -> bool {
    is_reserved(c) || is_unreserved(c)
}
fn is_pchar(c: char) -> bool {
    is_unreserved(c) || ":@&=+$,".contains(c)
}
fn is_path(c: char) -> bool {
    is_pchar(c) || ";/".contains(c)
}
fn is_userinfo(c: char) -> bool {
    is_unreserved(c) || ";:&=+$,".contains(c)
}
fn is_reg_name(c: char) -> bool {
    is_unreserved(c) || "$,;:@&=+".contains(c)
}
fn is_server(c: char) -> bool {
    is_userinfo(c) || is_alphanum(c) || c == '-' || ".:@[]".contains(c)
}
fn is_server_percent(c: char) -> bool {
    is_server(c) || c == '%'
}
fn is_scheme(c: char) -> bool {
    is_alpha(c) || is_digit(c) || "+-.".contains(c)
}
fn is_scope_id(c: char) -> bool {
    is_alphanum(c) || "_.".contains(c)
}
fn is_digit_or_dot(c: char) -> bool {
    is_digit(c) || c == '.'
}
fn is_alphanum_dash(c: char) -> bool {
    is_alphanum(c) || c == '-'
}

struct Parser {
    input: Vec<char>,
    uri: Uri,
    ipv6_byte_count: i32,
}

impl Parser {
    fn sub(&self, start: usize, end: usize) -> String {
        self.input[start..end].iter().collect()
    }
    fn at(&self, start: usize, end: usize, c: char) -> bool {
        start < end && self.input[start] == c
    }
    fn at_str(&self, start: usize, end: usize, s: &str) -> bool {
        let s: Vec<char> = s.chars().collect();
        start + s.len() <= end && self.input[start..start + s.len()] == s[..]
    }
    /// `scan(start, end, char c)`.
    fn scan_char(&self, start: usize, end: usize, c: char) -> usize {
        if start < end && self.input[start] == c {
            start + 1
        } else {
            start
        }
    }
    /// `scan(start, end, err, stop)`: `None` when an `err` char comes first.
    fn scan_stop(&self, start: usize, end: usize, err: &str, stop: &str) -> Option<usize> {
        let mut p = start;
        while p < end {
            let c = self.input[p];
            if err.contains(c) {
                return None;
            }
            if stop.contains(c) {
                break;
            }
            p += 1;
        }
        Some(p)
    }
    /// `scanEscape(start, n, first)`.
    fn scan_escape(&self, start: usize, n: usize, c: char) -> R<usize> {
        if c == '%' {
            if start + 3 <= n && is_hex(self.input[start + 1]) && is_hex(self.input[start + 2]) {
                return Ok(start + 3);
            }
            return Err(SyntaxError);
        }
        let u = c as u32;
        // Visible non-US-ASCII: not Character.isSpaceChar, not isISOControl.
        if u > 128 && !c.is_whitespace() && !(0x7f..=0x9f).contains(&u) {
            return Ok(start + 1);
        }
        Ok(start)
    }
    /// `scan(start, n, lowMask, highMask)`; `escaped` = mask has `L_ESCAPED`.
    fn scan(&self, start: usize, n: usize, matches: fn(char) -> bool, escaped: bool) -> R<usize> {
        let mut p = start;
        while p < n {
            let c = self.input[p];
            if (c as u32) < 128 && c != '\0' && matches(c) {
                p += 1;
                continue;
            }
            if escaped {
                let q = self.scan_escape(p, n, c)?;
                if q > p {
                    p = q;
                    continue;
                }
            }
            break;
        }
        Ok(p)
    }
    fn check_chars(&self, start: usize, end: usize, matches: fn(char) -> bool, escaped: bool) -> R<()> {
        if self.scan(start, end, matches, escaped)? < end {
            return Err(SyntaxError);
        }
        Ok(())
    }

    fn parse(&mut self) -> R<()> {
        let n = self.input.len();
        let p = self.scan_stop(0, n, "/?#", ":");
        let mut p = match p {
            Some(p) if self.at(p, n, ':') => {
                if p == 0 || !is_alpha(self.input[0]) {
                    return Err(SyntaxError);
                }
                self.check_chars(1, p, is_scheme, false)?;
                self.uri.scheme = Some(self.sub(0, p));
                let p = p + 1;
                if self.at(p, n, '/') {
                    self.parse_hierarchical(p, n)?
                } else {
                    let q = self.scan_stop(p, n, "", "#").unwrap_or(n);
                    if q == p {
                        return Err(SyntaxError);
                    }
                    self.check_chars(p, q, is_uric, true)?;
                    q
                }
            }
            _ => self.parse_hierarchical(0, n)?,
        };
        if self.at(p, n, '#') {
            self.check_chars(p + 1, n, is_uric, true)?;
            self.uri.fragment = Some(self.sub(p + 1, n));
            p = n;
        }
        if p < n {
            return Err(SyntaxError);
        }
        Ok(())
    }

    fn parse_hierarchical(&mut self, start: usize, n: usize) -> R<usize> {
        let mut p = start;
        if self.at(p, n, '/') && self.at(p + 1, n, '/') {
            p += 2;
            let q = self.scan_stop(p, n, "", "/?#").unwrap_or(n);
            if q > p {
                p = self.parse_authority(p, q)?;
            } else if q >= n {
                return Err(SyntaxError);
            }
        }
        let q = self.scan_stop(p, n, "", "?#").unwrap_or(n);
        self.check_chars(p, q, is_path, true)?;
        self.uri.path = Some(self.sub(p, q));
        p = q;
        if self.at(p, n, '?') {
            p += 1;
            let q = self.scan_stop(p, n, "", "#").unwrap_or(n);
            self.check_chars(p, q, is_uric, true)?;
            self.uri.query = Some(self.sub(p, q));
            p = q;
        }
        Ok(p)
    }

    fn parse_authority(&mut self, start: usize, n: usize) -> R<usize> {
        let p = start;
        let mut q = p;
        // Java tests `scan(p, n, "]") > p`: true unless the authority starts with ']'.
        let first_bracket = self.input[p..n].iter().position(|&c| c == ']').map_or(n, |i| p + i);
        let server_chars = if first_bracket > p {
            self.scan(p, n, is_server_percent, true)? == n
        } else {
            self.scan(p, n, is_server, true)? == n
        };
        let reg_chars = self.scan(p, n, is_reg_name, true)? == n;
        if reg_chars && !server_chars {
            self.uri.authority = Some(self.sub(p, n));
            return Ok(n);
        }
        if server_chars {
            match self.parse_server(p, n) {
                Ok(end) if end >= n => {
                    q = end;
                    self.uri.authority = Some(self.sub(p, n));
                }
                _ => {
                    self.uri.user_info = None;
                    self.uri.host = None;
                    self.uri.port = -1;
                    q = p;
                }
            }
        }
        if q < n {
            if reg_chars {
                self.uri.authority = Some(self.sub(p, n));
            } else {
                return Err(SyntaxError);
            }
        }
        Ok(n)
    }

    fn parse_server(&mut self, start: usize, n: usize) -> R<usize> {
        let mut p = start;
        if let Some(q) = self.scan_stop(p, n, "/?#", "@") {
            if q >= p && self.at(q, n, '@') {
                self.check_chars(p, q, is_userinfo, true)?;
                self.uri.user_info = Some(self.sub(p, q));
                p = q + 1;
            }
        }
        if self.at(p, n, '[') {
            p += 1;
            let q = self.scan_stop(p, n, "/?#", "]");
            match q {
                Some(q) if q > p && self.at(q, n, ']') => {
                    let r = self.scan_stop(p, q, "", "%").unwrap_or(q);
                    if r > p && r < q {
                        self.parse_ipv6_reference(p, r)?;
                        if r + 1 == q {
                            return Err(SyntaxError);
                        }
                        self.check_chars(r + 1, q, is_scope_id, false)?;
                    } else {
                        self.parse_ipv6_reference(p, q)?;
                    }
                    self.uri.host = Some(self.sub(p - 1, q + 1));
                    p = q + 1;
                }
                _ => return Err(SyntaxError),
            }
        } else {
            let q = self.parse_ipv4_address(p, n);
            let q = match q {
                Some(q) if q > p => q,
                _ => self.parse_hostname(p, n)?,
            };
            p = q;
        }
        if self.at(p, n, ':') {
            p += 1;
            let q = self.scan_stop(p, n, "", "/").unwrap_or(n);
            if q > p {
                self.check_chars(p, q, is_digit, false)?;
                let digits = self.sub(p, q);
                self.uri.port = digits.parse::<i32>().map_err(|_| SyntaxError)?;
                p = q;
            }
        }
        if p < n {
            return Err(SyntaxError);
        }
        Ok(p)
    }

    fn scan_byte(&self, start: usize, n: usize) -> R<usize> {
        let q = self.scan(start, n, is_digit, false)?;
        if q <= start {
            return Ok(q);
        }
        match self.sub(start, q).parse::<i32>() {
            Ok(v) if v > 255 => Ok(start),
            Ok(_) => Ok(q),
            Err(_) => Err(SyntaxError),
        }
    }

    /// `scanIPv4Address(start, n, strict)`: `Ok(None)` is Java's `-1`.
    fn scan_ipv4_address(&self, start: usize, n: usize, strict: bool) -> R<Option<usize>> {
        let mut p = start;
        let m = self.scan(p, n, is_digit_or_dot, false)?;
        if m <= p || (strict && m != n) {
            return Ok(None);
        }
        let mut steps = 0;
        loop {
            let q = if steps % 2 == 0 { self.scan_byte(p, m)? } else { self.scan_char(p, m, '.') };
            if q <= p {
                break;
            }
            p = q;
            steps += 1;
            if steps == 7 {
                if q < m {
                    break;
                }
                return Ok(Some(q));
            }
        }
        Err(SyntaxError)
    }

    fn parse_ipv4_address(&mut self, start: usize, n: usize) -> Option<usize> {
        let p = match self.scan_ipv4_address(start, n, false) {
            Ok(Some(p)) => p,
            _ => return None,
        };
        let p = if p > start && p < n && self.input[p] != ':' { return None } else { p };
        if p > start {
            self.uri.host = Some(self.sub(start, p));
        }
        Some(p)
    }

    fn parse_hostname(&mut self, start: usize, n: usize) -> R<usize> {
        let mut p = start;
        let mut l: Option<usize> = None;
        loop {
            let q = self.scan(p, n, is_alphanum, false)?;
            if q <= p {
                break;
            }
            l = Some(p);
            p = q;
            let q = self.scan(p, n, is_alphanum_dash, false)?;
            if q > p {
                if self.input[q - 1] == '-' {
                    return Err(SyntaxError);
                }
                p = q;
            }
            let q = self.scan_char(p, n, '.');
            if q <= p {
                break;
            }
            p = q;
            if p >= n {
                break;
            }
        }
        if p < n && !self.at(p, n, ':') {
            return Err(SyntaxError);
        }
        let l = l.ok_or(SyntaxError)?;
        if l > start && !is_alpha(self.input[l]) {
            return Err(SyntaxError);
        }
        self.uri.host = Some(self.sub(start, p));
        Ok(p)
    }

    fn parse_ipv6_reference(&mut self, start: usize, n: usize) -> R<usize> {
        let mut p = start;
        let mut compressed_zeros = false;
        let q = self.scan_hex_seq(p, n)?;
        if let Some(q) = q.filter(|&q| q > p) {
            p = q;
            if self.at_str(p, n, "::") {
                compressed_zeros = true;
                p = self.scan_hex_post(p + 2, n)?;
            } else if self.at(p, n, ':') {
                p = self.take_ipv4_address(p + 1, n)?;
                self.ipv6_byte_count += 4;
            }
        } else if self.at_str(p, n, "::") {
            compressed_zeros = true;
            p = self.scan_hex_post(p + 2, n)?;
        }
        if p < n {
            return Err(SyntaxError);
        }
        if self.ipv6_byte_count > 16 {
            return Err(SyntaxError);
        }
        if !compressed_zeros && self.ipv6_byte_count < 16 {
            return Err(SyntaxError);
        }
        if compressed_zeros && self.ipv6_byte_count == 16 {
            return Err(SyntaxError);
        }
        Ok(p)
    }

    fn scan_hex_post(&mut self, start: usize, n: usize) -> R<usize> {
        let mut p = start;
        if p == n {
            return Ok(p);
        }
        match self.scan_hex_seq(p, n)?.filter(|&q| q > p) {
            Some(q) => {
                p = q;
                if self.at(p, n, ':') {
                    p += 1;
                    p = self.take_ipv4_address(p, n)?;
                    self.ipv6_byte_count += 4;
                }
            }
            None => {
                p = self.take_ipv4_address(p, n)?;
                self.ipv6_byte_count += 4;
            }
        }
        Ok(p)
    }

    /// `scanHexSeq`: `Ok(None)` is Java's `-1`.
    fn scan_hex_seq(&mut self, start: usize, n: usize) -> R<Option<usize>> {
        let mut p = start;
        let q = self.scan(p, n, is_hex, false)?;
        if q <= p {
            return Ok(None);
        }
        if self.at(q, n, '.') {
            return Ok(None);
        }
        if q > p + 4 {
            return Err(SyntaxError);
        }
        self.ipv6_byte_count += 2;
        p = q;
        while p < n {
            if !self.at(p, n, ':') {
                break;
            }
            if self.at(p + 1, n, ':') {
                break;
            }
            p += 1;
            let q = self.scan(p, n, is_hex, false)?;
            if q <= p {
                return Err(SyntaxError);
            }
            if self.at(q, n, '.') {
                p -= 1;
                break;
            }
            if q > p + 4 {
                return Err(SyntaxError);
            }
            self.ipv6_byte_count += 2;
            p = q;
        }
        Ok(Some(p))
    }

    fn take_ipv4_address(&self, start: usize, n: usize) -> R<usize> {
        match self.scan_ipv4_address(start, n, true)? {
            Some(p) if p > start => Ok(p),
            _ => Err(SyntaxError),
        }
    }
}

/// `new URI(input)`.
pub fn parse(input: &str) -> Result<Uri, SyntaxError> {
    let mut parser = Parser { input: input.chars().collect(), uri: Uri { port: -1, ..Uri::default() }, ipv6_byte_count: 0 };
    parser.parse()?;
    Ok(parser.uri)
}
