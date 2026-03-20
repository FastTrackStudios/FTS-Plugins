//! NamGraph — node-based audio processing graph for building arbitrary
//! signal chains from NAM models, IR convolvers, and utility nodes.
//!
//! # Overview
//!
//! The graph is a directed acyclic graph (DAG) where each node processes
//! mono audio. Nodes are connected by edges that define signal flow.
//! Processing order is determined by topological sort.
//!
//! # Node types
//!
//! - **Input** — graph entry point (receives external audio)
//! - **Output** — graph exit point (produces final audio)
//! - **NamModel** — loads and processes a `.nam` model file
//! - **IrCab** — cabinet IR convolver
//! - **Gain** — simple gain/volume stage (linear)
//! - **Split** — duplicates signal to multiple outputs (implicit, via edges)
//! - **Mixer** — sums all incoming signals with per-input gain
//!
//! # Example: drive → preamp → amp → cab
//!
//! ```text
//! Input → [Drive.nam] → [Preamp.nam] → [Amp.nam] → [Mesa4x12.ir] → Output
//! ```
//!
//! # Example: 3 amps in parallel, blended
//!
//! ```text
//!            ┌→ [Fender.nam] ──┐
//! Input ─────┼→ [Marshall.nam] ┼→ [Mixer 0.33/0.33/0.33] → [IR] → Output
//!            └→ [Mesa.nam] ────┘
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::convolver::Convolver;
use crate::slot::NamSlot;

// ── Node types ─────────────────────────────────────────────────────

/// Unique node identifier.
pub type NodeId = u32;

/// Definition of a node in the graph (serializable for presets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDef {
    pub id: NodeId,
    pub kind: NodeKind,
    /// Human-readable label.
    #[serde(default)]
    pub label: String,
}

/// What kind of processing a node performs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    /// Graph input (exactly one per graph).
    Input,
    /// Graph output (exactly one per graph).
    Output,
    /// NAM model processor.
    NamModel {
        /// Path to the `.nam` file.
        path: String,
        /// Input gain (linear, default 1.0).
        #[serde(default = "one")]
        input_gain: f64,
        /// Whether to normalize output based on model loudness.
        #[serde(default)]
        normalize: bool,
    },
    /// Cabinet IR convolver.
    IrCab {
        /// Path to the IR file (raw f64 samples, WAV, etc.).
        path: String,
    },
    /// Simple gain stage.
    Gain {
        /// Gain in linear amplitude (default 1.0).
        #[serde(default = "one")]
        gain: f64,
    },
    /// Mixer — sums all incoming connections.
    /// Each input connection gets a weight (default 1/num_inputs).
    Mixer {
        /// Per-input weights. If empty, defaults to equal-weight sum.
        #[serde(default)]
        weights: Vec<f64>,
    },
}

fn one() -> f64 {
    1.0
}

/// A connection between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
}

/// Serializable graph definition (for presets/state save).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDef {
    pub nodes: Vec<NodeDef>,
    pub edges: Vec<Edge>,
}

// ── Runtime graph ──────────────────────────────────────────────────

/// Runtime state for a single node.
enum NodeState {
    Input,
    Output,
    NamModel(NamSlot),
    IrCab(Convolver),
    Gain { gain: f64 },
    Mixer { weights: Vec<f64> },
}

/// Live audio processing graph.
pub struct NamGraph {
    /// Node states in topological order.
    nodes: Vec<(NodeId, NodeState)>,
    /// Adjacency: from_id → [to_id, ...]
    edges_from: HashMap<NodeId, Vec<NodeId>>,
    /// Reverse adjacency: to_id → [from_id, ...]
    edges_to: HashMap<NodeId, Vec<NodeId>>,
    /// Per-node output buffer (mono).
    buffers: HashMap<NodeId, Vec<f64>>,
    /// Processing order (topologically sorted node IDs).
    order: Vec<NodeId>,
    /// Input node ID.
    input_id: Option<NodeId>,
    /// Output node ID.
    output_id: Option<NodeId>,

    sample_rate: f64,
    max_buffer_size: usize,
}

impl NamGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges_from: HashMap::new(),
            edges_to: HashMap::new(),
            buffers: HashMap::new(),
            order: Vec::new(),
            input_id: None,
            output_id: None,
            sample_rate: 48000.0,
            max_buffer_size: 512,
        }
    }

    /// Build the graph from a definition. Loads all models and IRs.
    pub fn build(
        &mut self,
        def: &GraphDef,
        sample_rate: f64,
        max_buffer_size: usize,
    ) -> Result<(), String> {
        self.sample_rate = sample_rate;
        self.max_buffer_size = max_buffer_size;
        self.nodes.clear();
        self.edges_from.clear();
        self.edges_to.clear();
        self.buffers.clear();
        self.input_id = None;
        self.output_id = None;

        // Create node states
        for node_def in &def.nodes {
            let state = match &node_def.kind {
                NodeKind::Input => {
                    if self.input_id.is_some() {
                        return Err("Multiple Input nodes".to_string());
                    }
                    self.input_id = Some(node_def.id);
                    NodeState::Input
                }
                NodeKind::Output => {
                    if self.output_id.is_some() {
                        return Err("Multiple Output nodes".to_string());
                    }
                    self.output_id = Some(node_def.id);
                    NodeState::Output
                }
                NodeKind::NamModel {
                    path,
                    input_gain,
                    normalize,
                } => {
                    let mut slot = NamSlot::new();
                    slot.input_gain = *input_gain;
                    slot.normalize = *normalize;
                    slot.load(path)?;
                    slot.update(sample_rate, max_buffer_size);
                    NodeState::NamModel(slot)
                }
                NodeKind::IrCab { path } => {
                    let mut conv = Convolver::new();
                    // For now, IR loading expects raw f64 samples.
                    // A real implementation would parse WAV files here.
                    // We store the path and load externally.
                    let _ = path; // Will be loaded via load_ir_for_node()
                    conv.reset();
                    NodeState::IrCab(conv)
                }
                NodeKind::Gain { gain } => NodeState::Gain { gain: *gain },
                NodeKind::Mixer { weights } => NodeState::Mixer {
                    weights: weights.clone(),
                },
            };

            self.buffers.insert(node_def.id, vec![0.0; max_buffer_size]);
            self.nodes.push((node_def.id, state));
        }

        if self.input_id.is_none() {
            return Err("No Input node".to_string());
        }
        if self.output_id.is_none() {
            return Err("No Output node".to_string());
        }

        // Build adjacency
        for edge in &def.edges {
            self.edges_from.entry(edge.from).or_default().push(edge.to);
            self.edges_to.entry(edge.to).or_default().push(edge.from);
        }

        // Topological sort (Kahn's algorithm)
        self.order = self.topological_sort()?;

        Ok(())
    }

    /// Load IR data for a specific IrCab node.
    pub fn load_ir_for_node(&mut self, node_id: NodeId, ir_data: &[f64]) -> Result<(), String> {
        for (id, state) in &mut self.nodes {
            if *id == node_id {
                match state {
                    NodeState::IrCab(conv) => {
                        conv.load_ir(ir_data, self.sample_rate);
                        return Ok(());
                    }
                    _ => return Err(format!("Node {node_id} is not an IrCab")),
                }
            }
        }
        Err(format!("Node {node_id} not found"))
    }

    /// Update a gain node's value at runtime.
    pub fn set_gain(&mut self, node_id: NodeId, gain: f64) {
        for (id, state) in &mut self.nodes {
            if *id == node_id {
                if let NodeState::Gain { gain: g } = state {
                    *g = gain;
                }
                return;
            }
        }
    }

    /// Update mixer weights at runtime.
    pub fn set_mixer_weights(&mut self, node_id: NodeId, weights: Vec<f64>) {
        for (id, state) in &mut self.nodes {
            if *id == node_id {
                if let NodeState::Mixer { weights: w } = state {
                    *w = weights;
                }
                return;
            }
        }
    }

    /// Process a block of mono audio through the graph.
    pub fn process(&mut self, input: &[f64], output: &mut [f64]) {
        let n = input.len().min(output.len());
        if n == 0 {
            return;
        }

        let input_id = match self.input_id {
            Some(id) => id,
            None => return,
        };
        let output_id = match self.output_id {
            Some(id) => id,
            None => return,
        };

        // Write input to the Input node's buffer
        if let Some(buf) = self.buffers.get_mut(&input_id) {
            buf[..n].copy_from_slice(&input[..n]);
        }

        // Process in topological order
        let order = self.order.clone();
        for &node_id in &order {
            if node_id == input_id {
                continue; // Input already filled
            }

            // Gather inputs from predecessor nodes
            let predecessors: Vec<NodeId> =
                self.edges_to.get(&node_id).cloned().unwrap_or_default();

            // Sum predecessor outputs into this node's buffer
            if !predecessors.is_empty() {
                // Find mixer weights if this is a Mixer node
                let weights = self.find_mixer_weights(node_id, predecessors.len());

                // Clear buffer
                if let Some(buf) = self.buffers.get_mut(&node_id) {
                    buf[..n].fill(0.0);
                }

                for (idx, &pred_id) in predecessors.iter().enumerate() {
                    let w = weights.get(idx).copied().unwrap_or(weights[0]);
                    // Copy pred buffer values (we need to borrow carefully)
                    let pred_samples: Vec<f64> = self
                        .buffers
                        .get(&pred_id)
                        .map(|b| b[..n].to_vec())
                        .unwrap_or_else(|| vec![0.0; n]);

                    if let Some(buf) = self.buffers.get_mut(&node_id) {
                        for i in 0..n {
                            buf[i] += pred_samples[i] * w;
                        }
                    }
                }
            }

            // Apply this node's processing in-place on its buffer
            self.process_node(node_id, n);
        }

        // Read output from the Output node's buffer
        if let Some(buf) = self.buffers.get(&output_id) {
            output[..n].copy_from_slice(&buf[..n]);
        }
    }

    fn find_mixer_weights(&self, node_id: NodeId, num_inputs: usize) -> Vec<f64> {
        for (id, state) in &self.nodes {
            if *id == node_id {
                if let NodeState::Mixer { weights } = state {
                    if !weights.is_empty() {
                        return weights.clone();
                    }
                }
                break;
            }
        }
        // Default: equal weight summing to 1.0
        let w = if num_inputs > 0 {
            1.0 / num_inputs as f64
        } else {
            1.0
        };
        vec![w; num_inputs.max(1)]
    }

    fn process_node(&mut self, node_id: NodeId, n: usize) {
        // Find the node index
        let idx = match self.nodes.iter().position(|(id, _)| *id == node_id) {
            Some(i) => i,
            None => return,
        };

        match &mut self.nodes[idx].1 {
            NodeState::Input | NodeState::Output => {
                // No processing — pass through
            }
            NodeState::NamModel(slot) => {
                // Get buffer, process in-place via a temp copy
                let buf = self.buffers.get(&node_id).unwrap();
                let input_copy: Vec<f64> = buf[..n].to_vec();
                let buf = self.buffers.get_mut(&node_id).unwrap();
                slot.process(&input_copy, &mut buf[..n]);
            }
            NodeState::IrCab(conv) => {
                let buf = self.buffers.get_mut(&node_id).unwrap();
                for i in 0..n {
                    buf[i] = conv.tick(buf[i]);
                }
            }
            NodeState::Gain { gain } => {
                let g = *gain;
                let buf = self.buffers.get_mut(&node_id).unwrap();
                for i in 0..n {
                    buf[i] *= g;
                }
            }
            NodeState::Mixer { .. } => {
                // Mixing already happened during input gathering
            }
        }
    }

    fn topological_sort(&self) -> Result<Vec<NodeId>, String> {
        let all_ids: Vec<NodeId> = self.nodes.iter().map(|(id, _)| *id).collect();
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();

        for &id in &all_ids {
            in_degree.insert(id, 0);
        }
        for edge_list in self.edges_from.values() {
            for &to in edge_list {
                *in_degree.entry(to).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<NodeId> = all_ids
            .iter()
            .filter(|id| in_degree[id] == 0)
            .copied()
            .collect();
        let mut sorted = Vec::new();

        while let Some(node) = queue.pop() {
            sorted.push(node);
            if let Some(successors) = self.edges_from.get(&node) {
                for &succ in successors {
                    let deg = in_degree.get_mut(&succ).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(succ);
                    }
                }
            }
        }

        if sorted.len() != all_ids.len() {
            return Err("Graph contains a cycle".to_string());
        }

        Ok(sorted)
    }

    /// Reset all node states (clears delay lines, model state, etc.)
    pub fn reset(&mut self) {
        for (_, state) in &mut self.nodes {
            match state {
                NodeState::IrCab(conv) => conv.reset(),
                _ => {}
            }
        }
        for buf in self.buffers.values_mut() {
            buf.fill(0.0);
        }
    }
}

impl Default for NamGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ── Builder helpers ────────────────────────────────────────────────

impl GraphDef {
    /// Create a simple serial chain: Input → nodes... → Output.
    ///
    /// Convenience for the common case of a linear signal path.
    /// Node IDs are assigned starting from 0 (Input) and 1 (Output),
    /// with processing nodes starting at 2.
    pub fn serial_chain(nodes: Vec<NodeKind>) -> Self {
        let mut defs = Vec::new();
        let mut edges = Vec::new();

        let input_id: NodeId = 0;
        let output_id: NodeId = 1;

        defs.push(NodeDef {
            id: input_id,
            kind: NodeKind::Input,
            label: "Input".into(),
        });
        defs.push(NodeDef {
            id: output_id,
            kind: NodeKind::Output,
            label: "Output".into(),
        });

        let mut prev_id = input_id;
        for (i, kind) in nodes.into_iter().enumerate() {
            let id = (i as NodeId) + 2;
            defs.push(NodeDef {
                id,
                kind,
                label: String::new(),
            });
            edges.push(Edge {
                from: prev_id,
                to: id,
            });
            prev_id = id;
        }
        // Connect last processing node to output
        edges.push(Edge {
            from: prev_id,
            to: output_id,
        });

        Self { nodes: defs, edges }
    }

    /// Create a parallel blend: Input → [nodes in parallel] → Mixer → Output.
    pub fn parallel_blend(nodes: Vec<NodeKind>, weights: Option<Vec<f64>>) -> Self {
        let mut defs = Vec::new();
        let mut edges = Vec::new();

        let input_id: NodeId = 0;
        let output_id: NodeId = 1;
        let mixer_id: NodeId = 2;

        defs.push(NodeDef {
            id: input_id,
            kind: NodeKind::Input,
            label: "Input".into(),
        });
        defs.push(NodeDef {
            id: output_id,
            kind: NodeKind::Output,
            label: "Output".into(),
        });
        defs.push(NodeDef {
            id: mixer_id,
            kind: NodeKind::Mixer {
                weights: weights.unwrap_or_default(),
            },
            label: "Mixer".into(),
        });

        for (i, kind) in nodes.into_iter().enumerate() {
            let id = (i as NodeId) + 3;
            defs.push(NodeDef {
                id,
                kind,
                label: String::new(),
            });
            // Input → parallel node
            edges.push(Edge {
                from: input_id,
                to: id,
            });
            // Parallel node → mixer
            edges.push(Edge {
                from: id,
                to: mixer_id,
            });
        }

        // Mixer → output
        edges.push(Edge {
            from: mixer_id,
            to: output_id,
        });

        Self { nodes: defs, edges }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    const SR: f64 = 48000.0;
    const BUF: usize = 512;

    fn sine(freq: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / SR).sin() * 0.5)
            .collect()
    }

    #[test]
    fn empty_graph_errors() {
        let mut g = NamGraph::new();
        let def = GraphDef {
            nodes: vec![],
            edges: vec![],
        };
        assert!(g.build(&def, SR, BUF).is_err());
    }

    #[test]
    fn input_output_only_passes_through() {
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![]);
        g.build(&def, SR, BUF).unwrap();

        let input = sine(440.0, 256);
        let mut output = vec![0.0; 256];
        g.process(&input, &mut output);

        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            assert!(
                (inp - out).abs() < 1e-10,
                "Passthrough mismatch at {i}: {inp} vs {out}"
            );
        }
    }

    #[test]
    fn gain_node_scales() {
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![NodeKind::Gain { gain: 0.5 }]);
        g.build(&def, SR, BUF).unwrap();

        let input = sine(440.0, 256);
        let mut output = vec![0.0; 256];
        g.process(&input, &mut output);

        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            assert!(
                (out - inp * 0.5).abs() < 1e-10,
                "Gain mismatch at {i}: expected {}, got {out}",
                inp * 0.5
            );
        }
    }

    #[test]
    fn serial_gains_multiply() {
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![
            NodeKind::Gain { gain: 0.5 },
            NodeKind::Gain { gain: 0.25 },
        ]);
        g.build(&def, SR, BUF).unwrap();

        let input = sine(440.0, 256);
        let mut output = vec![0.0; 256];
        g.process(&input, &mut output);

        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            let expected = inp * 0.5 * 0.25;
            assert!(
                (out - expected).abs() < 1e-10,
                "Serial gain mismatch at {i}: expected {expected}, got {out}"
            );
        }
    }

    #[test]
    fn parallel_mixer_averages() {
        let mut g = NamGraph::new();
        // Two parallel gain stages: 2.0 and 0.0, equal mix → should average to 1.0x
        let def = GraphDef::parallel_blend(
            vec![NodeKind::Gain { gain: 2.0 }, NodeKind::Gain { gain: 0.0 }],
            None, // Equal weights: 0.5 each
        );
        g.build(&def, SR, BUF).unwrap();

        let input = sine(440.0, 256);
        let mut output = vec![0.0; 256];
        g.process(&input, &mut output);

        // (input * 2.0) * 0.5 + (input * 0.0) * 0.5 = input * 1.0
        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            assert!(
                (out - inp).abs() < 1e-10,
                "Parallel mix mismatch at {i}: expected {inp}, got {out}"
            );
        }
    }

    #[test]
    fn parallel_weighted_mixer() {
        let mut g = NamGraph::new();
        // Two gains with custom weights
        let def = GraphDef::parallel_blend(
            vec![NodeKind::Gain { gain: 1.0 }, NodeKind::Gain { gain: 1.0 }],
            Some(vec![0.75, 0.25]),
        );
        g.build(&def, SR, BUF).unwrap();

        let input = sine(440.0, 256);
        let mut output = vec![0.0; 256];
        g.process(&input, &mut output);

        // Both gain=1.0, so output = 0.75*input + 0.25*input = input
        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            assert!(
                (out - inp).abs() < 1e-10,
                "Weighted mix mismatch at {i}: expected {inp}, got {out}"
            );
        }
    }

    #[test]
    fn ir_node_in_chain() {
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![NodeKind::IrCab {
            path: String::new(),
        }]);
        g.build(&def, SR, BUF).unwrap();

        // Load a unit impulse IR into node 2 (the IrCab)
        g.load_ir_for_node(2, &[1.0]).unwrap();

        let input = sine(440.0, 4096);
        let mut output = vec![0.0; 4096];

        // Process in blocks
        for chunk in (0..4096).step_by(BUF) {
            let end = (chunk + BUF).min(4096);
            g.process(&input[chunk..end], &mut output[chunk..end]);
        }

        // After IR latency, output should be non-trivial
        let tail_energy: f64 =
            output[1024..].iter().map(|s| s * s).sum::<f64>() / (4096 - 1024) as f64;
        assert!(
            tail_energy > 1e-6,
            "IR in chain should produce output: energy={tail_energy}"
        );
    }

    #[test]
    fn cycle_detection() {
        let mut g = NamGraph::new();
        let def = GraphDef {
            nodes: vec![
                NodeDef {
                    id: 0,
                    kind: NodeKind::Input,
                    label: "In".into(),
                },
                NodeDef {
                    id: 1,
                    kind: NodeKind::Output,
                    label: "Out".into(),
                },
                NodeDef {
                    id: 2,
                    kind: NodeKind::Gain { gain: 1.0 },
                    label: "A".into(),
                },
                NodeDef {
                    id: 3,
                    kind: NodeKind::Gain { gain: 1.0 },
                    label: "B".into(),
                },
            ],
            edges: vec![
                Edge { from: 0, to: 2 },
                Edge { from: 2, to: 3 },
                Edge { from: 3, to: 2 }, // cycle!
                Edge { from: 3, to: 1 },
            ],
        };
        let result = g.build(&def, SR, BUF);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cycle"));
    }

    #[test]
    fn runtime_gain_update() {
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![NodeKind::Gain { gain: 1.0 }]);
        g.build(&def, SR, BUF).unwrap();

        let input = vec![1.0; 64];
        let mut output = vec![0.0; 64];

        // Process at gain=1.0
        g.process(&input, &mut output);
        assert!((output[0] - 1.0).abs() < 1e-10);

        // Change gain to 0.5 at runtime
        g.set_gain(2, 0.5);
        g.process(&input, &mut output);
        assert!((output[0] - 0.5).abs() < 1e-10);
    }

    #[test]
    fn complex_graph_drive_preamp_amp_cab() {
        // Build: Input → Drive(2x) → Preamp(0.8x) → Amp(1.5x) → Cab(IR) → Output
        let mut g = NamGraph::new();
        let def = GraphDef::serial_chain(vec![
            NodeKind::Gain { gain: 2.0 }, // "drive"
            NodeKind::Gain { gain: 0.8 }, // "preamp"
            NodeKind::Gain { gain: 1.5 }, // "amp"
            NodeKind::IrCab {
                path: String::new(),
            }, // "cab"
        ]);
        g.build(&def, SR, BUF).unwrap();

        // Load IR for the cab (node 5)
        let ir: Vec<f64> = (0..256).map(|i| (-i as f64 / 50.0).exp()).collect();
        g.load_ir_for_node(5, &ir).unwrap();

        let input = sine(440.0, 4096);
        let mut output = vec![0.0; 4096];
        for chunk in (0..4096).step_by(BUF) {
            let end = (chunk + BUF).min(4096);
            g.process(&input[chunk..end], &mut output[chunk..end]);
        }

        for (i, &s) in output.iter().enumerate() {
            assert!(s.is_finite(), "NaN/Inf at sample {i}");
        }
    }

    #[test]
    fn serializable_roundtrip() {
        let def = GraphDef::serial_chain(vec![
            NodeKind::NamModel {
                path: "drive.nam".into(),
                input_gain: 1.5,
                normalize: true,
            },
            NodeKind::Gain { gain: 0.8 },
            NodeKind::IrCab {
                path: "mesa4x12.wav".into(),
            },
        ]);

        let json = serde_json::to_string_pretty(&def).unwrap();
        let restored: GraphDef = serde_json::from_str(&json).unwrap();

        assert_eq!(def.nodes.len(), restored.nodes.len());
        assert_eq!(def.edges.len(), restored.edges.len());
    }
}
