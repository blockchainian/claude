pub(super) fn format(text: &str) -> Option<String> {
    // syn drops ordinary comments. Quoted text may conservatively skip formatting.
    if text.contains("/*")
        || text.lines().any(|line| {
            let Some((prefix, comment)) = line.split_once("//") else {
                return false;
            };
            prefix.contains('"')
                || !(comment.starts_with('!')
                    || (comment.starts_with('/') && !comment.starts_with("//")))
        })
    {
        return None;
    }
    let file = syn::parse_file(text).ok()?;
    Some(prettyplease::unparse(&file))
}
