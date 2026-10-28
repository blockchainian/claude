use super::*;

#[test]
fn terminal_mermaid_diagrams_and_fallback() {
    let source = "```mermaid\ngraph LR\n A[Mermaid start] --> B[Mermaid finish]\n```";
    let mut records: Vec<serde_json::Value> = include_str!("../fixtures/transcript.jsonl")
        .lines()
        .take(2)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    records[0]["message"]["content"] = source.into();
    records[1]["message"]["content"][0]["text"] = source.into();
    let transcript = records
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    for installed in [true, false] {
        let path = tempfile::tempdir().unwrap();
        if installed {
            let binary = std::path::PathBuf::from(std::env::var_os("HOME").unwrap())
                .join("go/bin/mermaid-ascii");
            std::os::unix::fs::symlink(binary, path.path().join("mermaid-ascii")).unwrap();
        }
        let path_env = format!("PATH={}", path.path().display());
        let (_directory, tmux, _transcript) = start_probe_with_command(
            if installed {
                "mermaid-diagram"
            } else {
                "mermaid-fallback"
            },
            &transcript,
            |_| {},
            &["/usr/bin/env", &path_env],
        );
        let capture = tmux.capture_when(|capture| {
            if installed {
                capture.contains("│ Mermaid start") && capture.contains("│ Mermaid finish")
            } else {
                capture.contains("Install mermaid-ascii to render this diagram.")
            }
        });
        if installed {
            assert!(!capture.contains("graph LR"), "{capture}");
            tmux.run(&["resize-window", "-t", "probe", "-x", "72", "-y", "50"]);
            let narrow = tmux.capture_when(|capture| {
                let details = capture.lines().take(18).collect::<Vec<_>>().join("\n");
                details.contains("│ Mermaid start") && !details.contains("│ Mermaid finish")
            });
            assert!(!narrow.contains("graph LR"), "{narrow}");
            std::fs::write(
                std::env::temp_dir().join("agents-mermaid-narrow.txt"),
                narrow,
            )
            .unwrap();
        } else {
            assert!(capture.contains("graph LR"), "{capture}");
            assert!(capture.contains("A[Mermaid start]"), "{capture}");
        }
        std::fs::write(
            std::env::temp_dir().join(format!("agents-mermaid-{installed}.txt")),
            capture,
        )
        .unwrap();
    }
    let verdict =
        serde_json::json!({"verdict": "pass", "diagram": true, "clipping": true, "fallback": true});
    std::fs::write(
        std::env::temp_dir().join("agents-mermaid-probe.json"),
        serde_json::to_vec_pretty(&verdict).unwrap(),
    )
    .unwrap();
    println!("{verdict}");
}
