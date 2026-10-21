const MAGIC: &[u8; 4] = b"SLK1";

pub fn is_pair(state: &[u8]) -> bool {
    state.starts_with(MAGIC)
}

pub fn local(state: &[u8], player: u8) -> Option<Vec<u8>> {
    let mut rest = state.strip_prefix(MAGIC)?;
    let mut parts = Vec::with_capacity(2);
    for _ in 0..2 {
        let len = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        parts.push(rest.get(4..4 + len)?);
        rest = &rest[4 + len..];
    }
    rest.is_empty()
        .then(|| parts.get(usize::from(player)).map(|p| p.to_vec()))
        .flatten()
}

pub fn pair(state: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for _ in 0..2 {
        out.extend_from_slice(&(state.len() as u32).to_le_bytes());
        out.extend_from_slice(state);
    }
    out
}

fn bios(state: &[u8]) -> Option<u32> {
    let gba = if is_pair(state) {
        local(state, 0)?
    } else {
        state.to_vec()
    };
    if gba.len() >= 0x400 {
        Some(u32::from_le_bytes(gba[4..8].try_into().unwrap()))
    } else {
        None
    }
}

pub fn same_bios(a: &[u8], b: &[u8]) -> bool {
    match (bios(a), bios(b)) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_gives_each_player_the_state_back() {
        let pair = pair(b"gba");
        assert!(is_pair(&pair));
        assert_eq!(local(&pair, 0).as_deref(), Some(&b"gba"[..]));
        assert_eq!(local(&pair, 1).as_deref(), Some(&b"gba"[..]));
    }

    #[test]
    fn each_player_gets_their_own_gba() {
        let mut state = MAGIC.to_vec();
        for gba in [&b"zero"[..], &b"one!"[..]] {
            state.extend_from_slice(&4u32.to_le_bytes());
            state.extend_from_slice(gba);
        }
        assert_eq!(local(&state, 0).as_deref(), Some(&b"zero"[..]));
        assert_eq!(local(&state, 1).as_deref(), Some(&b"one!"[..]));
    }

    #[test]
    fn anything_but_a_whole_link_state_has_no_local_gba() {
        let whole = pair(b"gba");
        assert_eq!(local(b"a one-GBA state", 0), None);
        assert_eq!(local(&whole[..whole.len() - 1], 0), None);
        assert_eq!(local(&[whole.as_slice(), b"x"].concat(), 0), None);
        assert_eq!(local(&whole, 2), None);
    }

    fn gba(bios: u32) -> Vec<u8> {
        let mut state = vec![0u8; 0x400];
        state[4..8].copy_from_slice(&bios.to_le_bytes());
        state
    }

    #[test]
    fn states_on_the_same_bios_match_whether_paired_or_not() {
        assert!(same_bios(&pair(&gba(0xBAAE187F)), &gba(0xBAAE187F)));
        assert!(same_bios(&gba(1), &pair(&gba(1))));
    }

    #[test]
    fn states_on_different_bioses_do_not() {
        assert!(!same_bios(&pair(&gba(0xBAAE187F)), &pair(&gba(0xA6473709))));
    }

    #[test]
    fn a_state_too_short_to_be_a_gba_says_nothing() {
        assert!(same_bios(&0u64.to_le_bytes(), &1u64.to_le_bytes()));
    }
}
