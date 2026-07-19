use super::*;

/// Simple xorshift-like PRNG returning u32 values.
/// Using u32 output avoids precision-loss casts (u32 fits losslessly in f64).
fn next_rand_u32(state: &mut u64) -> u32 {
    *state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    u32::try_from(*state >> 33).unwrap_or(0)
}

/// Convert usize to f64 safely for small values (via u32 intermediate).
fn usize_as_f64(val: usize) -> f64 {
    f64::from(u32::try_from(val).unwrap_or(u32::MAX))
}

/// Brute-force: enumerate all subsets of {1..N-1}\Ω of size ≤ budget,
/// compute L(Γ) for each, return the minimum.
fn brute_force_optimal(tokens: &[u32], density_weights: &[f64], omega: &[usize], budget: usize) -> (Vec<usize>, f64) {
    let num_blocks = tokens.len();
    let ps = PrefixSums::build(tokens, density_weights);

    let omega_set: std::collections::BTreeSet<usize> = omega.iter().copied().collect();
    let candidates: Vec<usize> = (1..num_blocks).filter(|x| !omega_set.contains(x)).collect();

    let mut best_cost = f64::INFINITY;
    let mut best_gamma: Vec<usize> = vec![];

    let max_size = budget.min(candidates.len());
    for size in 0..=max_size {
        for combo in combinations(&candidates, size) {
            let mut bounds = Vec::with_capacity(omega.len().saturating_add(combo.len()).saturating_add(2));
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
        let rest = items.get(idx.saturating_add(1)..).unwrap_or_default();
        for mut sub in combinations(rest, count.saturating_sub(1)) {
            sub.insert(0, item);
            result.push(sub);
        }
    }
    result
}

// ── Brute-force agreement tests ─────────────────────────────────────

#[test]
fn brute_force_uniform_small() {
    for num_blocks in 6usize..=12 {
        let tokens: Vec<u32> = (1..=num_blocks).map(|i| u32::try_from(i).unwrap_or(0)).collect();
        let weights: Vec<f64> = vec![1.0f64; num_blocks];
        for k_val in 1..=3usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-9f64,
                "N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn brute_force_quadratic_density() {
    for num_blocks in 8usize..=14 {
        let tokens: Vec<u32> = vec![10; num_blocks];
        let weights: Vec<f64> = (1..=num_blocks).map(|i: usize| usize_as_f64(i.saturating_mul(i))).collect();
        for k_val in 1..=3usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-9f64,
                "N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn brute_force_with_omega() {
    let num_blocks = 10;
    let tokens: Vec<u32> = vec![5; num_blocks];
    let weights: Vec<f64> = vec![1.0f64; num_blocks];
    let omega = vec![3, 7];

    let dp_result = optimize_gamma(&tokens, &weights, &omega, 2);
    let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &omega, 2);

    assert!(
        (dp_result.cost - bf_cost).abs() < 1e-9f64,
        "With omega: DP cost {:.6} != BF cost {bf_cost:.6}",
        dp_result.cost
    );
    for &g_pos in &dp_result.gamma {
        assert!(!omega.contains(&g_pos), "Γ position {g_pos} overlaps Ω");
    }
}

#[test]
fn brute_force_random_densities() {
    let mut seed: u64 = 42;

    for num_blocks in 10usize..=18 {
        let tokens: Vec<u32> = std::iter::repeat_with(|| next_rand_u32(&mut seed).checked_rem(50).unwrap_or(0).max(1))
            .take(num_blocks)
            .collect();
        let weights: Vec<f64> =
            std::iter::repeat_with(|| f64::from(next_rand_u32(&mut seed)).max(0.01)).take(num_blocks).collect();

        for k_val in 1..=3usize.min(num_blocks.saturating_sub(1)) {
            let dp_result = optimize_gamma(&tokens, &weights, &[], k_val);
            let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &[], k_val);
            assert!(
                (dp_result.cost - bf_cost).abs() < 1e-6f64,
                "Random N={num_blocks}, K={k_val}: DP cost {:.6} != BF cost {bf_cost:.6}",
                dp_result.cost
            );
        }
    }
}

#[test]
fn brute_force_random_with_omega() {
    let mut seed: u64 = 1337;

    for num_blocks in 12usize..=16 {
        let tokens: Vec<u32> = std::iter::repeat_with(|| next_rand_u32(&mut seed).checked_rem(30).unwrap_or(0).max(1))
            .take(num_blocks)
            .collect();
        let weights: Vec<f64> =
            std::iter::repeat_with(|| f64::from(next_rand_u32(&mut seed)).max(0.01)).take(num_blocks).collect();

        let o1_raw = usize::try_from(
            next_rand_u32(&mut seed).checked_rem(u32::try_from(num_blocks.saturating_sub(1)).unwrap_or(1)).unwrap_or(0),
        )
        .unwrap_or(0);
        let o1_val = o1_raw.max(1).min(num_blocks.saturating_sub(2));
        let omega = vec![o1_val];

        let dp_result = optimize_gamma(&tokens, &weights, &omega, 2);
        let (_, bf_cost) = brute_force_optimal(&tokens, &weights, &omega, 2);
        assert!(
            (dp_result.cost - bf_cost).abs() < 1e-6f64,
            "Random+omega N={num_blocks}: DP cost {:.6} != BF cost {bf_cost:.6}",
            dp_result.cost
        );
    }
}

// ── Property tests ──────────────────────────────────────────────────

#[test]
fn cost_monotone_in_k() {
    let tokens: Vec<u32> = vec![10; 50];
    let weights: Vec<f64> = (1..=50usize).map(|i| usize_as_f64(i.saturating_mul(i))).collect();

    let mut prev_cost = f64::INFINITY;
    for k_val in 0..=5 {
        let result = optimize_gamma(&tokens, &weights, &[], k_val);
        assert!(
            result.cost <= prev_cost + 1e-9f64,
            "Cost increased: K={k_val} cost {:.6} > K={} cost {prev_cost:.6}",
            result.cost,
            k_val.saturating_sub(1)
        );
        prev_cost = result.cost;
    }
}

#[test]
fn gamma_disjoint_from_omega() {
    let tokens: Vec<u32> = vec![10; 20];
    let weights: Vec<f64> = vec![1.0f64; 20];
    let omega = vec![5, 10, 15];

    let result = optimize_gamma(&tokens, &weights, &omega, 3);
    for &g_pos in &result.gamma {
        assert!(!omega.contains(&g_pos), "Γ position {g_pos} overlaps Ω {omega:?}");
    }
}

#[test]
fn gamma_positions_in_range() {
    let tokens: Vec<u32> = vec![10; 30];
    let weights: Vec<f64> = vec![1.0f64; 30];

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    for &g_pos in &result.gamma {
        assert!((1..30).contains(&g_pos), "Γ position {g_pos} out of range [1, 29]");
    }
}

#[test]
fn gamma_sorted() {
    let tokens: Vec<u32> = vec![10; 50];
    let weights: Vec<f64> = (1..=50usize).map(|i| usize_as_f64(i.saturating_mul(i))).collect();

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    for pair in result.gamma.windows(2) {
        let left = pair.first().copied().unwrap_or(0);
        let right = pair.get(1).copied().unwrap_or(0);
        assert!(left < right, "Γ not sorted: {pair:?}");
    }
}

#[test]
fn tail_heavy_density_shifts_gamma_right() {
    let num_blocks = 100;
    let tokens: Vec<u32> = vec![10; num_blocks];
    let weights: Vec<f64> = (1..=num_blocks).map(|i| usize_as_f64(i).powi(4)).collect();

    let result = optimize_gamma(&tokens, &weights, &[], 3);
    assert_eq!(result.gamma.len(), 3);
    for &g_pos in &result.gamma {
        assert!(g_pos > 50, "With i^4 density, expected cuts in second half, got {g_pos}");
    }
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn empty_input() {
    let result = optimize_gamma(&[], &[], &[], 3);
    assert!(result.gamma.is_empty());
    assert!((result.cost - 0.0).abs() < 1e-9f64);
}

#[test]
fn single_block() {
    let result = optimize_gamma(&[42], &[1.0f64], &[], 3);
    assert!(result.gamma.is_empty());
    assert!((result.cost - 42.0).abs() < 1e-9f64);
}

#[test]
fn k_zero() {
    let tokens: Vec<u32> = vec![10; 10];
    let weights: Vec<f64> = vec![1.0f64; 10];
    let result = optimize_gamma(&tokens, &weights, &[], 0);
    assert!(result.gamma.is_empty());
    // Uniform: p_x = 0.1, T_x = 10x → L(∅) = Σ 0.1·10x = 55
    assert!((result.cost - 55.0).abs() < 1e-9f64, "K=0: cost {:.6} != expected 55.0", result.cost);
}

#[test]
fn k_exceeds_available_slots() {
    let tokens: Vec<u32> = vec![10; 5];
    let weights: Vec<f64> = vec![1.0f64; 5];
    let omega = vec![2, 4];

    let result = optimize_gamma(&tokens, &weights, &omega, 10);
    assert!(result.gamma.len() <= 2, "Too many cuts: {:?}", result.gamma);
}

#[test]
fn omega_at_every_position() {
    let num_blocks = 5;
    let tokens: Vec<u32> = vec![10; num_blocks];
    let weights: Vec<f64> = vec![1.0f64; num_blocks];
    let omega: Vec<usize> = (1..num_blocks).collect();

    let result = optimize_gamma(&tokens, &weights, &omega, 3);
    assert!(result.gamma.is_empty());
}

#[test]
fn two_blocks() {
    let tokens = vec![10u32, 20];
    let weights = vec![1.0f64, 1.0f64];

    // Without cut: p1·T1 + p2·T2 = 0.5·10 + 0.5·30 = 20
    let no_cut = optimize_gamma(&tokens, &weights, &[], 0);
    assert!((no_cut.cost - 20.0).abs() < 1e-9f64);

    // With cut at 1: p1·T1 + p2·(T2−T1) = 0.5·10 + 0.5·20 = 15
    let with_cut = optimize_gamma(&tokens, &weights, &[], 1);
    assert!((with_cut.cost - 15.0).abs() < 1e-9f64);
    assert_eq!(with_cut.gamma, vec![1]);
}

#[test]
fn deterministic() {
    let tokens: Vec<u32> = vec![5, 10, 15, 20, 25, 30];
    let weights: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];

    let r1 = optimize_gamma(&tokens, &weights, &[], 2);
    let r2 = optimize_gamma(&tokens, &weights, &[], 2);
    assert_eq!(r1.gamma, r2.gamma);
    assert!((r1.cost - r2.cost).abs() < 1e-15f64);
}
