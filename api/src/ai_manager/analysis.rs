//! Analysis Types
//!
//! Structures for representing AI analysis results and recommendations.

use serde::{Deserialize, Serialize};

/// AI analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Analysis {
    /// Brief summary of the analysis
    pub summary: String,
    /// Detailed reasoning for the recommendations
    pub reasoning: String,
    /// Recommended parameter changes
    pub recommendations: Vec<Recommendation>,
    /// Confidence level (0.0-1.0)
    pub confidence: f32,
}

/// A single parameter change recommendation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recommendation {
    /// Parameter path (e.g., "arena.max_wells")
    pub parameter: String,
    /// Recommended value
    pub value: f32,
    /// Reason for the recommendation
    pub reason: String,
}

impl Analysis {
    /// Create a "no action needed" analysis
    pub fn no_action(reason: &str) -> Self {
        Self {
            summary: "No action needed".to_string(),
            reasoning: reason.to_string(),
            recommendations: Vec::new(),
            confidence: 0.3,
        }
    }

    /// Check if this analysis recommends any changes
    pub fn has_recommendations(&self) -> bool {
        !self.recommendations.is_empty()
    }

    /// Check if confidence is above threshold
    pub fn is_confident(&self, threshold: f32) -> bool {
        self.confidence >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_action() {
        let analysis = Analysis::no_action("All metrics within target");

        assert_eq!(analysis.summary, "No action needed");
        assert!(!analysis.has_recommendations());
        assert!(!analysis.is_confident(0.5));
    }

    #[test]
    fn test_with_recommendations() {
        let analysis = Analysis {
            summary: "Performance degraded".to_string(),
            reasoning: "Tick time is high".to_string(),
            recommendations: vec![
                Recommendation {
                    parameter: "arena.max_wells".to_string(),
                    value: 15.0,
                    reason: "Reduce physics complexity".to_string(),
                },
            ],
            confidence: 0.85,
        };

        assert!(analysis.has_recommendations());
        assert!(analysis.is_confident(0.7));
        assert!(!analysis.is_confident(0.9));
    }
}
