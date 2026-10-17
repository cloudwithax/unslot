//! Finding a game on the card by name, from the shelf's X.

/// The most games a search lists.
pub const MOST: usize = 300;

/// Lower-case words of letters and digits, the rest dropped: `Pokémon: Emerald` is
/// `pokémon emerald`.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How well a name answers a query, lower being better, or None when it does not: every word
/// typed has to start a word of the name. A name that starts with the query comes first, then
/// one with a word that does.
pub fn rank(name: &str, query: &str) -> Option<u8> {
    let (name, query) = (words(name), words(query));
    if query.is_empty() {
        return None;
    }
    if !query
        .iter()
        .all(|q| name.iter().any(|w| w.starts_with(q.as_str())))
    {
        return None;
    }
    let joined = name.join(" ");
    Some(match () {
        _ if joined.starts_with(&query.join(" ")) => 0,
        _ if name
            .first()
            .is_some_and(|w| w.starts_with(query[0].as_str())) =>
        {
            1
        }
        _ => 2,
    })
}

/// The indices of the `names` that answer `query`, best first and otherwise in the order given.
pub fn matches<'a>(names: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    let mut found: Vec<(u8, usize)> = names
        .enumerate()
        .filter_map(|(i, name)| rank(name, query).map(|r| (r, i)))
        .collect();
    found.sort();
    found.into_iter().take(MOST).map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_typed_has_to_start_a_word_of_the_name() {
        assert_eq!(rank("The Legend of Zelda", "zel"), Some(2));
        assert_eq!(rank("The Legend of Zelda", "leg zel"), Some(2));
        assert_eq!(rank("The Legend of Zelda", "elda"), None);
        assert_eq!(rank("Metal Slug", "  "), None);
    }

    #[test]
    fn case_and_punctuation_do_not_matter() {
        assert_eq!(
            rank("Street Fighter II': Champion Edition", "street fighter ii"),
            Some(0)
        );
        assert_eq!(rank("Pokémon Emerald", "POKÉMON"), Some(0));
    }

    #[test]
    fn a_name_that_starts_with_the_query_comes_first() {
        let names = ["The Metal Slug Story", "Mega Man", "Metal Slug", "Metroid"];
        assert_eq!(matches(names.iter().copied(), "met"), [2, 3, 0]);
    }
}
