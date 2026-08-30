use std::collections::VecDeque;

#[derive(Debug)]
pub struct DiagnosticBuffer {
    entries: VecDeque<String>,
    retained_bytes: usize,
    max_bytes: usize,
}

impl DiagnosticBuffer {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            retained_bytes: 0,
            max_bytes,
        }
    }

    pub fn push(&mut self, entry: impl Into<String>) {
        if self.max_bytes == 0 {
            self.clear();
            return;
        }

        let mut entry = entry.into();
        if entry.len() > self.max_bytes {
            let keep_from = entry.len() - self.max_bytes;
            entry = String::from_utf8_lossy(&entry.as_bytes()[keep_from..]).into_owned();
        }

        self.retained_bytes += entry.len();
        self.entries.push_back(entry);

        while self.retained_bytes > self.max_bytes {
            if let Some(removed) = self.entries.pop_front() {
                self.retained_bytes = self.retained_bytes.saturating_sub(removed.len());
            } else {
                self.retained_bytes = 0;
                break;
            }
        }
    }

    #[cfg(test)]
    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.entries.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.retained_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::DiagnosticBuffer;

    #[test]
    fn evicts_old_entries_at_the_byte_limit() {
        let mut buffer = DiagnosticBuffer::new(6);
        buffer.push("first");
        buffer.push("two");
        buffer.push("last");

        assert_eq!(buffer.entries().collect::<Vec<_>>(), vec!["last"]);
    }

    #[test]
    fn retains_only_the_bounded_tail_of_a_large_entry() {
        let mut buffer = DiagnosticBuffer::new(4);
        buffer.push("123456");

        assert_eq!(buffer.entries().collect::<Vec<_>>(), vec!["3456"]);
    }

    #[test]
    fn a_zero_limit_retains_nothing() {
        let mut buffer = DiagnosticBuffer::new(0);
        buffer.push("discarded");

        assert_eq!(buffer.entries().count(), 0);
    }
}
