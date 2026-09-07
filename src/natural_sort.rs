#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortChunk {
    Num(u64),
    Str(String),
}

pub fn natural_sort_key(s: &str) -> Vec<SortChunk> {
    let mut chunks = Vec::new();
    let mut current_digits = String::new();
    let mut current_chars = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            if !current_chars.is_empty() {
                chunks.push(SortChunk::Str(current_chars.to_lowercase()));
                current_chars.clear();
            }
            current_digits.push(c);
        } else {
            if !current_digits.is_empty() {
                if let Ok(num) = current_digits.parse::<u64>() {
                    chunks.push(SortChunk::Num(num));
                } else {
                    chunks.push(SortChunk::Str(current_digits.clone()));
                }
                current_digits.clear();
            }
            current_chars.push(c);
        }
    }

    if !current_digits.is_empty() {
        if let Ok(num) = current_digits.parse::<u64>() {
            chunks.push(SortChunk::Num(num));
        } else {
            chunks.push(SortChunk::Str(current_digits));
        }
    }
    if !current_chars.is_empty() {
        chunks.push(SortChunk::Str(current_chars.to_lowercase()));
    }

    chunks
}

pub fn compare_natural(a: &str, b: &str) -> std::cmp::Ordering {
    natural_sort_key(a).cmp(&natural_sort_key(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_natural_sort_order() {
        let mut list = vec!["item10.jpg", "item2.jpg", "item1.jpg", "item20.jpg"];
        list.sort_by(|a, b| compare_natural(a, b));
        assert_eq!(list, vec!["item1.jpg", "item2.jpg", "item10.jpg", "item20.jpg"]);
    }
}
