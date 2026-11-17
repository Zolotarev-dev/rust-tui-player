//! Sanitization utilities for safely rendering untrusted strings in the TUI.

/// Remove ANSI escape sequences and control characters from `input`.
///
/// - Strips CSI sequences like `ESC [ ... <final byte>`
/// - Strips OSC sequences like `ESC ] ... (BEL | ST)`
/// - Strips other `ESC`-introduced sequences conservatively
/// - Removes all Unicode control characters (`char::is_control()`), including
///   C0 controls (`U+0000..=U+001F`, `U+007F`) and C1 controls (`U+0080..=U+009F`),
///   while preserving intended printable Unicode text.
pub fn sanitize_for_display(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\x1b' {
            // ESC sequence
            i += 1;
            if i >= chars.len() {
                break;
            }
            match chars[i] {
                '[' => {
                    // CSI: ESC [ params/intermediates final-byte(0x40..=0x7E)
                    i += 1;
                    while i < chars.len() {
                        let c = chars[i] as u32;
                        i += 1;
                        if (0x40..=0x7E).contains(&c) {
                            break;
                        }
                    }
                }
                ']' => {
                    // OSC: ESC ] ... terminated by BEL or ST (ESC \)
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\x07' {
                            i += 1;
                            break;
                        }
                        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                'P' | 'X' | '^' | '_' => {
                    // DCS/SOS/PM/APC: terminated by ST (ESC \)
                    i += 1;
                    while i < chars.len() {
                        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Other escape: skip the introducer char as well
                    i += 1;
                }
            }
            continue;
        }

        // Strip all Unicode control characters (C0, DEL, C1, etc.)
        if ch.is_control() {
            i += 1;
            continue;
        }

        out.push(ch);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sanitize_for_display;

    #[test]
    fn strips_ansi_csi_sequences() {
        let input = "Hello \x1b[31mRed\x1b[0m World";
        assert_eq!(sanitize_for_display(input), "Hello Red World");
    }

    #[test]
    fn strips_control_characters() {
        let input = "A\x00B\x07C\x1fD\x7fE\nF\tG";
        assert_eq!(sanitize_for_display(input), "ABCDEFG");
    }

    #[test]
    fn leaves_normal_unicode() {
        let input = "Música — トラック 🎵";
        assert_eq!(sanitize_for_display(input), "Música — トラック 🎵");
    }

    #[test]
    fn strips_osc_sequence_bel() {
        let input = "title\x1b]0;evil\x07ok";
        assert_eq!(sanitize_for_display(input), "titleok");
    }

    #[test]
    fn strips_osc_sequence_st() {
        let input = "title\x1b]0;evil\x1b\\ok";
        assert_eq!(sanitize_for_display(input), "titleok");
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(sanitize_for_display(""), "");
    }

    #[test]
    fn strips_standalone_esc_at_end() {
        let input = "abc\x1b";
        assert_eq!(sanitize_for_display(input), "abc");
    }

    #[test]
    fn strips_c1_controls() {
        let input = "A\u{0085}B\u{009B}C\u{009D}D\u{009C}E";
        assert_eq!(sanitize_for_display(input), "ABCDE");
    }

    #[test]
    fn strips_c1_control_range() {
        // Sample from U+0080..U+009F range
        let input = "X\u{0080}\u{008D}\u{0090}\u{009F}Y";
        assert_eq!(sanitize_for_display(input), "XY");
    }
}
