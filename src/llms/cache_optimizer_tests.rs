use super::*;

/// Brute-force: enumerate all subsets of {1..N-1}\Ω of size ≤ budget,
/// compute L(Γ) for each, return the minimum.
fn brute_force_optimal(tokens: &[u32], density_weights: &[f64], omega: &[usize], budget: usize) -> (Vec<usize>, f64) {
    let num_blocks = tokens.len();
    let ps = PrefixSums::build(tokens, density_weights);

    let omega_set: std::collections::HashSet<usize> = omega.iter().copied().collect();
    let candidates: Vec<usize> = (1..num_blocks).filter(|x| !omega_set.contains(x)).collect();

    let mut best_cost = f64::INFINITY;
    let mut best_gamma: Vec<usize> = vec![];

    let max_size = budget.min(candidates.len());
    for size in 0..=max_size {
        for combo in combinations(&candidates, size) {
            let mut bounds = Vec::with_capacity(omega.len() + combo.len() + 2);
            bounds.push(0);
            bounds.extend_from_slice(omega);
            bounds.extend_from_slice(&combo);
            bounds.push(num_blocks);
            bounds.sort_unstable();
            bounds.dedup();

            let cost: f64 = bounds
                .windows(2)
                .map(|win| {
                    let left = win.first().copied().unwrap_or(0);
                    let right = win.get(1).copied().unwrap_or(0);
                    ps.block_cost(left, right)
                })
                .sum();

            if cost < best_cost {
                best_cost = cost;
                best_gamma = combo;
            }
        }
    }

    (best_gamma, best_cost)
}

/// Generate all combinations of `count` elements from `items`.
fn combinations(items: &[usize], count: usize) -> Vec<Vec<usize>> {
    if count == 0 {
        return vec![vec![]];
    }
    if items.len() < count {
        return vec![];
    }
    let mut result = Vec::new();
    for (idx, &item) in items.iter().enumerate() {
        let rest = &items[idx.saturating_add(1)..];
        for mut sub in combinations(rest, count.saturating_sub(1)) {
            sub.insert(0, item);
            result.push(sub);
        }
    }
    result
}

// ── Brute-force agreement tests ─────────────────────────────────────

#[test]
fn test_brute_force_uniform_small() {
    for num_blocks in 6_usize..=12 {
        let tokens: Vec<u32> = (1..=num_blocks).map(|i| i as u32).collect();
        let weights: Vec<f64> = vec![1.0; num_blocks];
        for k_val in 1..=3_usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-9,
                "N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn test_brute_force_quadratic_density() {
    for num_blocks in 8_usize..=14 {
        let tokens: Vec<u32> = vec![10; num_blocks];
        let weights: Vec<f64> = (1..=num_blocks).map(|i: usize| (i.saturating_mul(i)) as f64).collect();
        for k_val in 1..=3_usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-9,
                "N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn test_brute_force_with_omega() {
    let num_blocks = 10;
    let tokens: Vec<u32> = vec![5; num_blocks];
    let weights: Vec<f64> = vec![1.0; num_blocks];
    let omega = vec![3, 7];

    let dp_result = optimize_gamma(&tokens, &weights, &omega, 2);
    let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &omega, 2);

    assert!(
        (dp_result.cost - bf_cost).abs() < 1e-9,
        "With omega: DP cost {:.6} != BF cost {bf_cost:.6}",
        dp_result.cost
    );
    for &g_pos in &dp_result.gamma {
        assert!(!omega.contains(&g_pos), "Γ position {g_pos} overlaps Ω");
    }
}

#[test]
fn test_brute_force_random_densities() {
    let mut seed: u64 = 42;
    let next_rand = |state: &mut u64| -> f64 {
        *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (*state >> 33) as f64 / (u32::MAX as f64)
    };

    for num_blocks in 10_usize..=18 {
        let tokens: Vec<u32> = (0..num_blocks)
            .map(|_| {
                let val = (next_rand(&mut seed) * 50.0) as u32;
                val.max(1)
            })
            .collect();
        let weights: Vec<f64> = (0..num_blocks).map(|_| next_rand(&mut seed).max(0.01)).collect();

        for k_val in 1..=3_usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-6,
                "Random N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn test_brute_force_random_with_omega() {
    let mut seed: u64 = 1337;
    let next_rand = |state: &mut u64| -> f64 {
        *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (*state >> 33) as f64 / (u32::MAX as f64)
    };

    for num_blocks in 12_usize..=16 {
        let tokens: Vec<u32> = (0..num_blocks).map(|_| ((next_rand(&mut seed) * 30.0) as u32).max(1)).collect();
        let weights: Vec<f64> = (0..num_blocks).map(|_| next_rand(&mut seed).max(0.01)).collect();

        let o1_raw = (next_rand(&mut seed) * num_blocks.saturating_sub(1) as f64) as usize;
        let o1_val = o1_raw.max(1).min(num_blocks.saturating_sub(2));
        let omega = vec![o1_val];

        let dp_result = optimize_gamma(&tokens, &weights, &omega, 2);
        let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &omega, 2);
        assert!(
            (dp_result.cost - bf_cost).abs() < 1e-6,
            "Random+omega N={num_blocks}: DP cost {:.6} != BF cost {bf_cost:.6}",
            dp_result.cost
        );
    }
}

// ── Property tests ──────────────────────────────────────────────────

#[test]
fn test_cost_monotone_in_k() {
    let tokens: Vec<u32> = vec![10; 50];
    let weights: Vec<f64> = (1..=50_usize).map(|i| (i.saturating_mul(i)) as f64).collect();

    let mut prev_cost = f64::INFINITY;
    for k_val in 0..=5 {
        let result = optimize_gamma(&tokens, &weights, &[], k_val);
        assert!(
            result.cost <= prev_cost + 1e-9,
            "Cost increased: K={k_val} cost {:.6} > K={} cost {prev_cost:.6}",
            result.cost,
            k_val.saturating_sub(1)
        );
        prev_cost = result.cost;
    }
}

#[test]
fn test_gamma_disjoint_from_omega() {
    let tokens: Vec<u32> = vec![10; 20];
    let weights: Vec<f64> = vec![1.0; 20];
    let omega = vec![5, 10, 15];

    let result = optimize_gamma(&tokens, &weights, &omega, 3);
    for &g_pos in &result.gamma {
        assert!(!omega.contains(&g_pos), "Γ position {g_pos} overlaps Ω {omega:?}");
    }
}

#[test]
fn test_gamma_positions_in_range() {
    let tokens: Vec<u32> = vec![10; 30];
    let weights: Vec<f64> = vec![1.0; 30];

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    for &g_pos in &result.gamma {
        assert!(g_pos >= 1 && g_pos < 30, "Γ position {g_pos} out of range [1, 29]");
    }
}

#[test]
fn test_gamma_sorted() {
    let tokens: Vec<u32> = vec![10; 50];
    let weights: Vec<f64> = (1..=50_usize).map(|i| (i.saturating_mul(i)) as f64).collect();

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    for pair in result.gamma.windows(2) {
        let left = pair.first().copied().unwrap_or(0);
        let right = pair.get(1).copied().unwrap_or(0);
        assert!(left < right, "Γ not sorted: {pair:?}");
    }
}

#[test]
fn test_tail_heavy_density_shifts_gamma_right() {
    let num_blocks = 100;
    let tokens: Vec<u32> = vec![10; num_blocks];
    let weights: Vec<f64> = (1..=num_blocks).map(|i| (i as f64).powi(4)).collect();

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    assert_eq!(result.gamma.len(), 3);
    for &g_pos in &result.gamma {
        assert!(g_pos > 50, "With i^4 density, expected cuts in second half, got {g_pos}");
    }
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn test_empty_input() {
    let result = optimize_gamma(&[], &[], &[], 3);
    assert!(result.gamma.is_empty());
    assert!((result.cost - 0.0).abs() < 1e-9);
}

#[test]
fn test_single_block() {
    let result = optimize_gamma(&[42], &[1.0], &[], 3);
    assert!(result.gamma.is_empty());
    assert!((result.cost - 42.0).abs() < 1e-9);
}

#[test]
fn test_k_zero() {
    let tokens: Vec<u32> = vec![10; 10];
    let weights: Vec<f64> = vec![1.0; 10];
    let result = optimize_gamma(&tokens, &weights, &[], 0);
    assert!(result.gamma.is_empty());
    // Uniform: p_x = 0.1, T_x = 10x → L(∅) = Σ 0.1·10x = 55
    assert!((result.cost - 55.0).abs() < 1e-9, "K=0: cost {:.6} != expected 55.0", result.cost);
}

#[test]
fn test_k_exceeds_available_slots() {
    let tokens: Vec<u32> = vec![10; 5];
    let weights: Vec<f64> = vec![1.0; 5];
    let omega = vec![2, 4];

    let result = optimize_gamma(&tokens, &weights, &omega, 10);
    assert!(result.gamma.len() <= 2, "Too many cuts: {:?}", result.gamma);
}

#[test]
fn test_omega_at_every_position() {
    let num_blocks = 5;
    let tokens: Vec<u32> = vec![10; num_blocks];
    let weights: Vec<f64> = vec![1.0; num_blocks];
    let omega: Vec<usize> = (1..num_blocks).collect();

    let result = optimize_gamma(&tokens, &weights, &omega, 3);
    assert!(result.gamma.is_empty());
}

#[test]
fn test_two_blocks() {
    let tokens = vec![10_u32, 20];
    let weights = vec![1.0, 1.0];

    // Without cut: p1·T1 + p2·T2 = 0.5·10 + 0.5·30 = 20
    let no_cut = optimize_gamma(&tokens, &weights, &[], 0);
    assert!((no_cut.cost - 20.0).abs() < 1e-9);

    // With cut at 1: p1·T1 + p2·(T2−T1) = 0.5·10 + 0.5·20 = 15
    let with_cut = optimize_gamma(&tokens, &weights, &[], 1);
    assert!((with_cut.cost - 15.0).abs() < 1e-9);
    assert_eq!(with_cut.gamma, vec![1]);
}

#[test]
fn test_deterministic() {
    let tokens: Vec<u32> = vec![5, 10, 15, 20, 25, 30];
    let weights: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];

    let r1 = optimize_gamma(&tokens, &weights, &[], 2);
    let r2 = optimize_gamma(&tokens, &weights, &[], 2);
    assert_eq!(r1.gamma, r2.gamma);
    assert!((r1.cost - r2.cost).abs() < 1e-15);
}
