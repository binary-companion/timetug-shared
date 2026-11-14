//! Duplicate detection: the pair rules, the resolver, the user's lessons and the model-verdict types.

/// Model-verdict types: adjudication requests, answers, events and the verdict cache.
pub mod adjudication;
/// The user's merge and separate corrections ("lessons").
pub mod lesson_book;
/// `DuplicateResolver`, which merges duplicate events from every source.
pub mod resolver;
/// The pair rules that decide whether two events are duplicates.
pub mod rules;
