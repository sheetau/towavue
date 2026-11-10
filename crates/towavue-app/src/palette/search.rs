use super::*;
use std::collections::BTreeMap;
use towavue_core::CommandDefinition;

mod keywords;

pub(super) fn title(command: &CommandDefinition, language: localization::Language) -> &str {
    options::title(command.id, language).unwrap_or_else(|| match command.id {
        CommandId::ToggleImageInterpolation => Text::ImageInterpolation.in_language(language),
        CommandId::ToggleImageMinification => Text::ImageMinification.in_language(language),
        _ => command.title_in(language),
    })
}

fn terms(text: &str) -> BTreeMap<String, usize> {
    let mut terms = BTreeMap::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if !word.is_empty() {
            *terms.entry(word.to_lowercase()).or_default() += 1;
        }
    }
    terms
}

// Direct matches retain MRU priority. Every command supplies search phrases;
// ranking is shared and never dispatches based on the query itself.
pub(super) fn ranked(
    query: &str,
    language: localization::Language,
    history: &[CommandId],
) -> (Vec<&'static CommandDefinition>, usize, usize) {
    let definitions: Vec<_> = command_definitions()
        .iter()
        .filter(|definition| !options::child(definition.id))
        .collect();
    let labels: Vec<_> = definitions
        .iter()
        .map(|definition| {
            let title = title(definition, language).to_lowercase();
            let alias = title_for_english(definition).to_lowercase();
            (title, alias)
        })
        .collect();
    let direct: Vec<_> = labels
        .iter()
        .map(|(label, alias)| label.contains(query) || alias.contains(query))
        .collect();
    let mut matches: Vec<_> = definitions
        .iter()
        .enumerate()
        .filter(|(index, _)| direct[*index])
        .map(|(_, definition)| *definition)
        .collect();
    matches.sort_by_key(|definition| {
        history
            .iter()
            .position(|id| *id == definition.id)
            .unwrap_or(usize::MAX)
    });
    let recent_count = matches
        .iter()
        .take_while(|definition| history.contains(&definition.id))
        .count();
    let direct_count = matches.len();
    let query_terms = terms(query);
    if !query_terms.is_empty() && query.chars().count() >= if query.is_ascii() { 3 } else { 2 } {
        let documents: Vec<_> = labels
            .iter()
            .zip(&definitions)
            .map(|((label, alias), definition)| {
                terms(&format!(
                    "{label} {alias} {}",
                    keywords::phrases(definition.id)
                ))
            })
            .collect();
        let weights: Vec<_> = query_terms
            .keys()
            .map(|term| {
                let frequency = documents
                    .iter()
                    .filter(|doc| doc.contains_key(term))
                    .count();
                (
                    term,
                    1.0 + ((documents.len() + 1) as f64 / (frequency + 1) as f64).ln(),
                )
            })
            .collect();
        let total: f64 = weights.iter().map(|(_, weight)| weight).sum();
        let mut scores: Vec<_> = documents
            .iter()
            .enumerate()
            .filter(|(index, _)| !direct[*index])
            .filter_map(|(index, doc)| {
                let mut score = 0.0;
                for (term, weight) in &weights {
                    let best = doc
                        .keys()
                        .map(|word| similarity(term, word))
                        .fold(0.0_f64, f64::max);
                    // Do not ignore an unmatched direction or qualifier: "skip back"
                    // must not qualify a forward command just because "skip" matches.
                    if best == 0.0 {
                        return None;
                    }
                    score += weight * best;
                }
                score /= total;
                let exact_phrase = keywords::phrases(definitions[index].id)
                    .split('|')
                    .any(|phrase| terms(phrase) == query_terms);
                if exact_phrase {
                    score += 0.15;
                }
                (score >= 0.8).then_some((index, score))
            })
            .collect();
        scores.sort_by(|(a, sa), (b, sb)| {
            sb.total_cmp(sa)
                .then_with(|| labels[*a].0.cmp(&labels[*b].0))
        });
        let maximum = scores.first().map_or(0.0, |(_, score)| *score);
        for (index, _) in scores
            .into_iter()
            .filter(|(_, score)| *score >= maximum * 0.65)
            .take(5)
        {
            matches.push(definitions[index]);
        }
    }
    (matches, recent_count, direct_count)
}

fn similarity(query: &str, word: &str) -> f64 {
    if query == word {
        return 1.0;
    }
    // Japanese terms are matched as whole phrases/substrings, never by a
    // one-character edit that would turn an unrelated short word into a hit.
    if !query.is_ascii() {
        return if query.chars().count() >= 2 && word.contains(query) {
            0.9
        } else {
            0.0
        };
    }
    if query.len() < 4 || !word.is_ascii() || query.len().abs_diff(word.len()) > 1 {
        return 0.0;
    }
    let (a, b) = (query.as_bytes(), word.as_bytes());
    let first = a
        .iter()
        .zip(b)
        .position(|(a, b)| a != b)
        .unwrap_or(a.len().min(b.len()));
    // Four-letter substitutions confuse ordinary words (save/same, file/fine).
    // Keep adjacent transpositions at that length, but require five for replacement.
    let close = if a.len() == b.len() {
        (a.len() >= 5 && a.get(first + 1..) == b.get(first + 1..))
            || (first + 1 < a.len()
                && a[first] == b[first + 1]
                && a[first + 1] == b[first]
                && a[first + 2..] == b[first + 2..])
    } else if a.len() < b.len() {
        a[first..] == b[first + 1..]
    } else {
        a[first + 1..] == b[first..]
    };
    if close { 0.8 } else { 0.0 }
}

fn title_for_english(command: &CommandDefinition) -> &str {
    title(command, localization::Language::English)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vocabulary_covers_actions_languages_directions_and_numeric_qualifiers() {
        for language in [
            localization::Language::English,
            localization::Language::Japanese,
        ] {
            for (query, expected) in [
                ("fast", CommandId::RateUp),
                ("speed", CommandId::RateUp),
                ("speed up", CommandId::RateUp),
                ("slow down", CommandId::RateDown),
                ("skip", CommandId::SeekForward),
                ("skip back", CommandId::SeekBackward),
                ("skip ahead", CommandId::SeekForward),
                ("倍速", CommandId::RateUp),
                ("巻き戻し", CommandId::SeekBackward),
                ("crop selection", CommandId::KeepTimeSelection),
                ("mute selection", CommandId::SilenceTimeSelection),
                ("hotkeys", CommandId::OpenKeyboardSettings),
                ("explorer", CommandId::RevealFile),
                ("snapshot", CommandId::ExportFrame),
                ("comic", CommandId::ToggleReadingMode),
                ("captions", CommandId::LoadSubtitles),
                ("random", CommandId::ToggleAudioShuffle),
                ("soundtrack", CommandId::CycleAudioTrack),
                ("原寸", CommandId::ActualSize),
                ("revert edit", CommandId::Undo),
                ("clipboard", CommandId::PasteImage),
                ("jump to 30", CommandId::SeekVideo30),
            ] {
                let (matches, _, direct) = ranked(query, language, &[]);
                assert!(
                    matches.iter().any(|command| command.id == expected),
                    "{language:?}: {query} -> {expected:?}"
                );
                assert!(matches.len() - direct <= 5);
            }
            for (query, rejected) in [
                ("skip back", CommandId::SeekForward),
                ("speed up", CommandId::RateDown),
                ("slow down", CommandId::RateUp),
                ("jump to 30", CommandId::SeekVideo40),
            ] {
                assert!(
                    !ranked(query, language, &[])
                        .0
                        .iter()
                        .any(|command| command.id == rejected),
                    "{query}"
                );
            }
            for query in ["speed banana", "skip zzzzzzz", "weather forecast"] {
                assert!(ranked(query, language, &[]).0.is_empty(), "{query}");
            }
        }
    }

    #[test]
    fn semantic_and_typo_suggestions_are_bounded_and_preserve_direct_priority() {
        for (query, expected) in [
            ("save", CommandId::ExportAudio),
            ("export", CommandId::Save),
            ("exprot", CommandId::ExportAudio),
            ("remove", CommandId::DeleteFile),
            ("mirror", CommandId::FlipHorizontal),
            ("保存", CommandId::ExportAs),
        ] {
            let (matches, _, direct) = ranked(query, localization::Language::English, &[]);
            assert!(
                matches[direct..].iter().any(|entry| entry.id == expected),
                "{query}"
            );
            assert!(matches.len() - direct <= 5);
            assert!(
                matches[direct..]
                    .iter()
                    .all(|entry| !matches[..direct].iter().any(|first| first.id == entry.id))
            );
        }
        for query in ["zzzzzz", "save zzzzzzzzz", "sv", "pl"] {
            let (matches, _, direct) = ranked(query, localization::Language::English, &[]);
            assert_eq!(matches.len(), direct, "{query}");
        }
        let (matches, recent, direct) =
            ranked("save", localization::Language::English, &[CommandId::Save]);
        assert_eq!(matches[0].id, CommandId::Save);
        assert_eq!(recent, 1);
        assert!(direct >= recent);
        assert!(!matches[direct..].iter().any(|entry| matches!(
            entry.id,
            CommandId::NextSameKind | CommandId::PreviousSameKind | CommandId::ToggleReuseFolderTab
        )));
        assert_eq!(similarity("save", "same"), 0.0);
        assert_eq!(similarity("svae", "save"), 0.8);
    }

    #[test]
    fn suggestions_follow_direct_matches_and_ignore_history_rank() {
        let (matches, recent, direct) = ranked(
            "open file",
            localization::Language::English,
            &[CommandId::ReopenClosedTab],
        );
        assert_eq!(matches[0].id, CommandId::OpenFile);
        assert_eq!(recent, 0);
        assert!(direct > 0 && matches.len() > direct);
        assert!(matches.len() - direct <= 5);
        let (matches, _, direct) = ranked("file open", localization::Language::English, &[]);
        assert_eq!(direct, 0);
        assert!(
            matches
                .iter()
                .any(|command| command.id == CommandId::OpenFile)
        );
        assert!(
            ranked("zzzzzz", localization::Language::English, &[])
                .0
                .is_empty()
        );
        let (matches, _, direct) = ranked("op", localization::Language::English, &[]);
        assert_eq!(matches.len(), direct);
    }
}
