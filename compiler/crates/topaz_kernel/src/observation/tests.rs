use super::comparison::*;
use super::*;

#[cfg(test)]
mod comparison_tests {
    use super::*;

    #[test]
    fn each_semantic_phase_mutation_is_first_gated_and_canonical() {
        for (phase, path) in [
            ("source-set", "source-set.jsonl"),
            ("tokens", "tokens.jsonl"),
            ("ast", "ast.jsonl"),
            ("resolved", "resolved.jsonl"),
            ("typed", "typed.jsonl"),
            ("lowered", "lowered.jsonl"),
            ("diagnostics", "diagnostics.jsonl"),
            ("outcome", "response/outcome"),
        ] {
            let left = [ComparedMember {
                path,
                bytes: b"{\"rowKind\":\"left\",\"sourceId\":\"s:left\"}\n",
            }];
            let right = [ComparedMember {
                path,
                bytes: b"{\"rowKind\":\"right\",\"sourceId\":\"s:right\"}\n",
            }];
            let record = compare_phase(ComparisonLayer::Semantic, phase, left, right)
                .expect("mutation differs");
            assert_eq!(record.first_failing_phase.as_deref(), Some(phase));
            assert_eq!(record.mismatch_count, 1);
            crate::canonical::validate(&record.bytes, false)
                .expect("comparison record is canonical");
        }
    }

    #[test]
    fn mismatch_output_is_bounded_deterministic_and_reports_total() {
        let paths = (0..40)
            .map(|index| format!("sources/{index:06}.tpz"))
            .collect::<Vec<_>>();
        let left_bytes = (0..40)
            .map(|index| format!("left-{index}").into_bytes())
            .collect::<Vec<_>>();
        let right_bytes = (0..40)
            .map(|index| format!("right-{index}").into_bytes())
            .collect::<Vec<_>>();
        let left = paths
            .iter()
            .zip(&left_bytes)
            .map(|(path, bytes)| ComparedMember { path, bytes })
            .collect::<Vec<_>>();
        let right = paths
            .iter()
            .zip(&right_bytes)
            .map(|(path, bytes)| ComparedMember { path, bytes })
            .collect::<Vec<_>>();
        let first = compare_phase(
            ComparisonLayer::Semantic,
            "source-set",
            left.clone(),
            right.clone(),
        )
        .expect("different");
        let second =
            compare_phase(ComparisonLayer::Semantic, "source-set", left, right).expect("different");
        assert_eq!(first, second);
        assert_eq!(first.mismatch_count, 40);
        let text = std::str::from_utf8(&first.bytes).expect("UTF-8");
        assert!(text.contains("\"truncated\":true"), "{text}");
        assert_eq!(text.matches("\"phase\":\"source-set\"").count(), 32);
    }

    #[test]
    fn native_binary_layer_compares_exact_bytes() {
        let equal = compare_native_binaries(b"same", b"same");
        assert!(equal.equal);
        let mismatch = compare_native_binaries(b"left", b"right");
        assert!(!mismatch.equal);
        assert_eq!(
            mismatch.first_failing_phase.as_deref(),
            Some("native-binary")
        );
        assert_eq!(mismatch.mismatch_count, 1);
    }
}
