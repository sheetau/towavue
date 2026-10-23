use super::*;
use std::collections::BTreeMap;
use towavue_core::CommandDefinition;

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
        if word.chars().count() >= 3 {
            *terms.entry(word.to_lowercase()).or_default() += 1;
        }
    }
    terms
}

/// Direct matches retain MRU order. Like VS Code's command provider, TF-IDF
/// suggestions use a three-character minimum, a > 0.5 normalized score and a
/// five-result budget across the catalog, and appear after direct matches.
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
    if query.chars().count() >= 3 {
        let documents: Vec<_> = labels
            .iter()
            .map(|(label, alias)| {
                terms(&if label == alias {
                    label.clone()
                } else {
                    format!("{label} {alias}")
                })
            })
            .collect();
        let mut weights = Vec::new();
        for (term, count) in terms(query) {
            let frequency = documents
                .iter()
                .filter(|doc| doc.contains_key(&term))
                .count();
            if frequency > 0 {
                let idf = ((documents.len() + 1) as f64 / frequency as f64).ln();
                weights.push((term, count as f64 * idf * idf));
            }
        }
        let mut scores: Vec<_> = documents
            .iter()
            .enumerate()
            .map(|(index, doc)| {
                let score = weights
                    .iter()
                    .map(|(term, weight)| *weight * doc.get(term).copied().unwrap_or(0) as f64)
                    .sum::<f64>();
                (index, score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();
        scores.sort_by(|(a, sa), (b, sb)| {
            sb.total_cmp(sa)
                .then_with(|| labels[*a].0.cmp(&labels[*b].0))
        });
        let maximum = scores.first().map_or(0.0, |(_, score)| *score);
        for (index, _) in scores
            .into_iter()
            .filter(|(_, score)| *score > maximum * 0.5)
            .take(5)
        {
            if !direct[index] {
                matches.push(definitions[index]);
            }
        }
    }
    (matches, recent_count, direct_count)
}

fn title_for_english(command: &CommandDefinition) -> &str {
    title(command, localization::Language::English)
}

#[cfg(test)]
mod tests {
    use super::*;
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
