use chess::Color;

pub(crate) fn budget_ms(words: &[&str], side: Color, overhead: u64) -> Option<u64> {
    let number = |key: &str| {
        words
            .windows(2)
            .find(|w| w[0] == key)
            .and_then(|w| w[1].parse::<u64>().ok())
    };
    if words.contains(&"infinite") {
        return None;
    }
    if let Some(ms) = number("movetime") {
        return Some(ms.saturating_sub(overhead).max(1));
    }
    let (clock, inc) = if side == Color::White {
        (number("wtime"), number("winc"))
    } else {
        (number("btime"), number("binc"))
    };
    clock.map(|remaining| {
        let moves = number("movestogo").unwrap_or(30).clamp(1, 100);
        (remaining / moves)
            .saturating_add(inc.unwrap_or(0) / 2)
            .min(remaining.saturating_sub(overhead))
            .max(1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budgets_respect_clock_and_overhead() {
        assert_eq!(
            budget_ms(&["go", "movetime", "100"], Color::White, 20),
            Some(80)
        );
        assert_eq!(
            budget_ms(
                &["go", "wtime", "1000", "btime", "2000", "movestogo", "10"],
                Color::White,
                20
            ),
            Some(100)
        );
        assert_eq!(budget_ms(&["go", "btime", "5"], Color::Black, 20), Some(1));
        assert_eq!(
            budget_ms(&["go", "infinite", "wtime", "100"], Color::White, 20),
            None
        );
    }
}
