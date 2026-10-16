//! The sampler chain of a chat: repeat penalty → top-k → top-p → min-p →
//! temperature → distribution (the llama.cpp order).
//!
//! The truncating stages are implemented here rather than with llama.cpp's
//! samplers: `TokenDataArray::as_mut_ptr` doesn't carry the C array's new
//! `size` back to the Rust vector, so a C sampler that drops candidates
//! leaves stale entries behind. Temperature and the final draw don't change
//! the number of candidates and use llama.cpp.

use std::collections::{HashSet, VecDeque};

use skelm_llama_cpp::{
    Sampler, SamplerChain, SamplerDistance, SamplerGreedy, SamplerTemperature, Token,
    TokenDataArray,
};

use crate::store::Sampling;

pub fn build(sampling: &Sampling) -> SamplerChain {
    let mut chain = SamplerChain::new();
    if sampling.repeat_penalty > 0.0 && (sampling.repeat_penalty - 1.0).abs() > f32::EPSILON {
        chain.add(Box::new(RepeatPenalty::new(
            sampling.repeat_penalty,
            sampling.repeat_last_n as usize,
        )));
    }
    if sampling.temperature <= 0.0 {
        chain.add(Box::new(SamplerGreedy));
        return chain;
    }
    if sampling.top_k > 0 {
        chain.add(Box::new(TopK(sampling.top_k as usize)));
    }
    if sampling.top_p > 0.0 && sampling.top_p < 1.0 {
        chain.add(Box::new(TopP(sampling.top_p)));
    }
    if sampling.min_p > 0.0 {
        chain.add(Box::new(MinP(sampling.min_p)));
    }
    chain.add(Box::new(SamplerTemperature::new(sampling.temperature)));
    let seed = sampling.seed.unwrap_or(0xFFFF_FFFF); // LLAMA_DEFAULT_SEED: random
    chain.add(Box::new(SamplerDistance::new(seed)));
    chain
}

fn sort_desc(array: &mut TokenDataArray) {
    if !array.sorted {
        array
            .data
            .sort_unstable_by(|a, b| b.logit().total_cmp(&a.logit()));
        array.sorted = true;
    }
}

/// Penalize the tokens among the last `n` generated ones
pub struct RepeatPenalty {
    penalty: f32,
    n: usize,
    last: VecDeque<Token>,
}

impl RepeatPenalty {
    pub fn new(penalty: f32, n: usize) -> Self {
        Self {
            penalty,
            n,
            last: VecDeque::with_capacity(n),
        }
    }
}

impl Sampler for RepeatPenalty {
    fn accept(&mut self, token: Token) {
        if self.n == 0 {
            return;
        }
        if self.last.len() == self.n {
            self.last.pop_front();
        }
        self.last.push_back(token);
    }

    fn apply(&mut self, array: &mut TokenDataArray) {
        if self.last.is_empty() {
            return;
        }
        let seen: HashSet<usize> = self.last.iter().map(|t| t.as_index()).collect();
        for d in array.data.iter_mut() {
            if seen.contains(&d.id().as_index()) {
                let logit = d.logit();
                d.set_logit(if logit > 0.0 {
                    logit / self.penalty
                } else {
                    logit * self.penalty
                });
            }
        }
        array.sorted = false;
    }

    fn reset(&mut self) {
        self.last.clear();
    }
}

/// Keep the `k` most likely candidates
pub struct TopK(pub usize);

impl Sampler for TopK {
    fn accept(&mut self, _: Token) {}
    fn reset(&mut self) {}

    fn apply(&mut self, array: &mut TokenDataArray) {
        let k = self.0;
        if k == 0 || k >= array.data.len() {
            return;
        }
        if !array.sorted {
            array
                .data
                .select_nth_unstable_by(k - 1, |a, b| b.logit().total_cmp(&a.logit()));
            array.data.truncate(k);
        } else {
            array.data.truncate(k);
        }
        array.selected = None;
    }
}

/// Keep the smallest set of most likely candidates whose probabilities add
/// up to `p`
pub struct TopP(pub f32);

impl Sampler for TopP {
    fn accept(&mut self, _: Token) {}
    fn reset(&mut self) {}

    fn apply(&mut self, array: &mut TokenDataArray) {
        if array.data.len() <= 1 {
            return;
        }
        sort_desc(array);
        let max = array.data[0].logit();
        let exps: Vec<f32> = array.data.iter().map(|d| (d.logit() - max).exp()).collect();
        let total: f32 = exps.iter().sum();
        let mut cumulative = 0.0;
        let mut keep = array.data.len();
        for (i, e) in exps.iter().enumerate() {
            cumulative += e / total;
            if cumulative >= self.0 {
                keep = i + 1;
                break;
            }
        }
        array.data.truncate(keep.max(1));
        array.selected = None;
    }
}

/// Drop the candidates less likely than `p` times the most likely one
pub struct MinP(pub f32);

impl Sampler for MinP {
    fn accept(&mut self, _: Token) {}
    fn reset(&mut self) {}

    fn apply(&mut self, array: &mut TokenDataArray) {
        if array.data.len() <= 1 {
            return;
        }
        let max = array
            .data
            .iter()
            .map(|d| d.logit())
            .fold(f32::NEG_INFINITY, f32::max);
        let min_logit = max + self.0.ln();
        array.data.retain(|d| d.logit() >= min_logit);
        array.selected = None;
    }
}
