//! Reading a growing JSON Lines transcript by its complete lines: forward from an offset, or
//! backward from its end.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// Bytes read per step, so a large transcript is never held whole.
const CHUNK: usize = 8 << 20;

/// Bytes read per step backward, where the lines wanted usually sit near the end.
const BACK_CHUNK: u64 = 64 << 10;

/// Detects overwrites at the previously read boundary before treating growth as an append.
#[derive(Default)]
pub(crate) struct Boundary {
    offset: u64,
    bytes: Vec<u8>,
}

impl Boundary {
    pub fn matches(&self, file: &mut File) -> io::Result<bool> {
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = vec![0; self.bytes.len()];
        file.read_exact(&mut bytes)?;
        Ok(bytes == self.bytes)
    }

    pub fn capture(&mut self, file: &mut File, end: u64) -> io::Result<()> {
        self.offset = end.saturating_sub(128);
        file.seek(SeekFrom::Start(self.offset))?;
        self.bytes.resize((end - self.offset) as usize, 0);
        file.read_exact(&mut self.bytes)
    }
}

/// Hands `visit` each complete line from `from` on and returns the offset after the last one; an
/// unterminated last line waits for its newline.
pub fn read_lines(file: &mut File, from: u64, mut visit: impl FnMut(&[u8])) -> io::Result<u64> {
    file.seek(SeekFrom::Start(from))?;
    let mut read = from;
    let mut pending = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            return Ok(read);
        }
        pending.extend_from_slice(&chunk[..count]);
        let Some(end) = pending.iter().rposition(|&byte| byte == b'\n') else {
            continue;
        };
        for line in pending[..end].split(|&byte| byte == b'\n') {
            visit(line);
        }
        read += end as u64 + 1;
        pending.drain(..=end);
    }
}

/// Hands `visit` the complete lines before `end` newest first, with the offset each starts at,
/// until it returns false.
pub fn read_lines_back(
    file: &mut File,
    end: u64,
    mut visit: impl FnMut(u64, &[u8]) -> bool,
) -> io::Result<()> {
    let mut position = end;
    // Bytes from `position` up to the line that `visit` saw last.
    let mut rest = Vec::new();
    while position > 0 {
        let amount = position.min(BACK_CHUNK);
        position -= amount;
        file.seek(SeekFrom::Start(position))?;
        let mut bytes = vec![0; amount as usize];
        file.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&rest);
        // The first line is whole only at the start of the file.
        let first = if position == 0 {
            0
        } else if let Some(newline) = bytes.iter().position(|&byte| byte == b'\n') {
            newline + 1
        } else {
            rest = bytes;
            continue;
        };
        let mut line_end = bytes.len();
        for start in (first..bytes.len())
            .rev()
            .filter(|&at| at == first || bytes[at - 1] == b'\n')
        {
            let line = &bytes[start..line_end];
            let line = line.strip_suffix(b"\n").unwrap_or(line);
            if !line.is_empty() && !visit(position + start as u64, line) {
                return Ok(());
            }
            line_end = start;
        }
        rest = bytes[..first].to_vec();
    }
    Ok(())
}

/// The offset after the file's last newline, where its complete lines end.
pub fn complete_end(file: &mut File) -> io::Result<u64> {
    let mut position = file.metadata()?.len();
    while position > 0 {
        let amount = position.min(BACK_CHUNK);
        position -= amount;
        file.seek(SeekFrom::Start(position))?;
        let mut bytes = vec![0; amount as usize];
        file.read_exact(&mut bytes)?;
        if let Some(newline) = bytes.iter().rposition(|&byte| byte == b'\n') {
            return Ok(position + newline as u64 + 1);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file(text: &str) -> (tempfile::NamedTempFile, File) {
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        write!(temp, "{text}").unwrap();
        let file = File::open(temp.path()).unwrap();
        (temp, file)
    }

    #[test]
    fn forward_reading_stops_before_an_unterminated_line() {
        let (_temp, mut file) = file("a\nbc\nd");
        let mut lines = Vec::new();
        let end = read_lines(&mut file, 2, |line| lines.push(line.to_vec())).unwrap();
        assert_eq!(lines, [b"bc".to_vec()]);
        assert_eq!(end, 5);
    }

    #[test]
    fn backward_reading_gives_lines_newest_first_with_their_offsets() {
        let (_temp, mut file) = file("a\nbc\n\nd\ne");
        let end = complete_end(&mut file).unwrap();
        assert_eq!(end, 8);
        let mut lines = Vec::new();
        read_lines_back(&mut file, end, |at, line| {
            lines.push((at, line.to_vec()));
            true
        })
        .unwrap();
        assert_eq!(
            lines,
            [(6, b"d".to_vec()), (2, b"bc".to_vec()), (0, b"a".to_vec())]
        );
    }

    #[test]
    fn backward_reading_joins_lines_across_chunks_and_stops_when_asked() {
        let long = "x".repeat(BACK_CHUNK as usize + 10);
        let (_temp, mut file) = file(&format!("first\n{long}\nlast\n"));
        let end = complete_end(&mut file).unwrap();
        let mut lines = Vec::new();
        read_lines_back(&mut file, end, |at, line| {
            lines.push((at, line.len()));
            lines.len() < 2
        })
        .unwrap();
        assert_eq!(lines, [(end - 5, 4), (6, long.len())]);
    }
}
