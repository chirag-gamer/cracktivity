//! Prompts & budgets (spec §6.4): short task-specific system prompts (SLM
//! quality degrades with complex prompts), quote/signature stripping before
//! summarization, an ~8k-token context budget with chunking for long threads.

use super::Msg;

/// Approximate token budget for thread context: 8k-token context window
/// minus prompt/answer headroom. ~4 chars per token for English.
const CONTEXT_CHARS: usize = 24_000;

/// Strips quoted reply lines (`>`-prefixed) and common signature markers —
/// threads shrink ~3–5× before summarization (spec §6.4).
pub fn strip_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('>') {
            continue;
        }
        let lower = t.to_lowercase();
        if lower == "--"
            || lower.starts_with("-- ")
            || lower.starts_with("-----original message-----")
            || lower.starts_with("----- forwarded message")
            || lower.starts_with("von:")
            || lower.starts_with("from:")
                && (lower.contains("sent:") || lower.contains("geschrieben:"))
        {
            // Signature delimiter and forwarded headers end the fresh
            // content: everything below is quoted boilerplate.
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// Builds the summarize request: transcript in, concise summary out. Long
/// transcripts are chunked (map-reduce): each chunk gets its own summary
/// pass, and the chunk summaries are reduced into the final one.
pub fn summarize_messages(transcript: &str) -> Vec<Vec<Msg>> {
    let clean = strip_quotes(transcript);
    let system = "Summarize the email thread. 2-4 sentences: what it is about, \
what each participant said or asked, any decision or open question. Plain text.";
    if clean.chars().count() <= CONTEXT_CHARS {
        return vec![vec![
            Msg::system(system),
            Msg::user(format!("Thread:\n\n{clean}")),
        ]];
    }
    // Map phase: ~equal chunks, cut at line boundaries.
    let chunks = split_into_chunks(&clean, CONTEXT_CHARS);
    let map_system = "Summarize this part of an email thread in 1-2 sentences. Plain text.";
    let chunk_msgs: Vec<Vec<Msg>> = chunks
        .iter()
        .map(|c| {
            vec![
                Msg::system(map_system),
                Msg::user(format!("Thread part:\n\n{c}")),
            ]
        })
        .collect();
    // Reduce phase runs in the caller with the final "reduce" prompt.
    chunk_msgs
}

/// The reduce prompt for chunk summaries.
pub fn summarize_reduce_messages(chunk_summaries: &[String]) -> Vec<Msg> {
    vec![
        Msg::system(
            "Combine these part-summaries of one email thread into one summary. \
2-4 sentences: what it is about, what was said, decisions or open questions. Plain text.",
        ),
        Msg::user(chunk_summaries.join("\n\n")),
    ]
}

/// Builds the reply-draft request: thread context + short instruction in,
/// an editable draft out. The user always sends it themselves (FR-22 AC).
pub fn draft_messages(transcript: &str, instruction: &str, my_address: &str) -> Vec<Msg> {
    let clean = strip_quotes(transcript);
    let mut context = clean.clone();
    if context.chars().count() > CONTEXT_CHARS {
        // Keep the tail: the latest messages matter most for a reply.
        let skip = context.chars().count() - CONTEXT_CHARS;
        context = context.chars().skip(skip).collect();
    }
    vec![
        Msg::system(format!(
            "You draft email replies for {my_address}. Write ONLY the reply body \
(plain text, no greeting line with the recipient name, no signature). Match the thread's \
tone and language. Never invent facts."
        )),
        Msg::user(format!(
            "Thread so far:\n\n{context}\n\nInstruction: {instruction}\n\nDraft the reply now."
        )),
    ]
}

fn split_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut len = 0usize;
    for line in text.lines() {
        let line_len = line.chars().count() + 1;
        if len + line_len > max_chars && len > 0 {
            chunks.push(std::mem::take(&mut current));
            len = 0;
        }
        current.push_str(line);
        current.push('\n');
        len += line_len;
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-21 AC: quotes/signatures shrink before the model sees the thread.
    #[test]
    fn quotes_and_signatures_are_stripped() {
        let raw = "Real content here.\n> quoted nonsense\n> more quoting\n-- \nJohn Doe\nSenior VP";
        let stripped = strip_quotes(raw);
        assert_eq!(stripped, "Real content here.");
    }

    /// FR-21 AC: the ≤ 8k context budget — a short thread is one pass; a long
    /// one is chunked into map passes (then reduced).
    #[test]
    fn long_threads_chunk_within_budget() {
        let short = summarize_messages("hello there");
        assert_eq!(short.len(), 1);

        let long_line = "lorem ipsum dolor sit amet ".repeat(40); // ~1 KB
        let long = (0..40)
            .map(|_| long_line.clone())
            .collect::<Vec<_>>()
            .join("\n");
        let passes = summarize_messages(&long);
        assert!(passes.len() > 1, "chunked: {}", passes.len());
        for pass in &passes {
            let user_chars: usize = pass.iter().map(|m| m.content.chars().count()).sum();
            assert!(
                user_chars <= CONTEXT_CHARS + 1100,
                "chunk within budget + one line: {user_chars}"
            );
        }
        // The reduce prompt combines the map outputs.
        let reduce = summarize_reduce_messages(&["a".into(), "b".into()]);
        assert_eq!(reduce.len(), 2);
    }

    /// FR-22: the draft prompt carries the (tail-truncated) thread + the
    /// user's instruction, and addresses the user explicitly.
    #[test]
    fn draft_prompt_shape() {
        let msgs = draft_messages("we discussed the deadline", "polite nudge", "me@x.com");
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].content.contains("me@x.com"));
        assert!(msgs[1].content.contains("polite nudge"));
        assert!(msgs[1].content.contains("we discussed the deadline"));
    }
}
