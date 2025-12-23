use crate::net::protocol::PlayerInput;
use crate::util::vec2::Vec2;

/// Violations detected by the anti-cheat system
#[derive(Debug, Clone, thiserror::Error)]
pub enum CheatViolation {
    #[error("Invalid thrust vector magnitude: {0}")]
    InvalidThrust(f32),
    #[error("Invalid aim vector magnitude: {0}")]
    InvalidAim(f32),
    #[error("Input from future tick: client={0}, server={1}")]
    FutureInput(u64, u64),
    #[error("Input too stale: client={0}, server={1}, max_delay={2}")]
    StaleInput(u64, u64, u64),
    #[error("NaN or Infinity in input values")]
    InvalidFloats,
    #[error("Sequence number went backwards: prev={0}, current={1}")]
    SequenceRegression(u64, u64),
    #[error("Sequence jumped too far: prev={0}, current={1}")]
    SequenceJump(u64, u64),
}

/// Configuration for input validation
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Maximum allowed vector magnitude (with tolerance)
    pub max_vector_magnitude: f32,
    /// Maximum ticks ahead of server time
    pub max_future_ticks: u64,
    /// Maximum ticks behind server time (plus RTT compensation)
    pub max_stale_ticks: u64,
    /// Maximum sequence jump allowed
    pub max_sequence_jump: u64,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_vector_magnitude: 1.001, // Slight tolerance for float precision
            max_future_ticks: 2,
            max_stale_ticks: 30, // ~1 second at 30 Hz
            max_sequence_jump: 100,
        }
    }
}

/// Input validator for anti-cheat
pub struct InputValidator {
    config: ValidationConfig,
}

impl InputValidator {
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }

    /// Validate a player input
    pub fn validate_input(&self, input: &PlayerInput) -> Result<(), CheatViolation> {
        self.validate_vector_values(input)?;
        self.validate_thrust(&input.thrust)?;
        self.validate_aim(&input.aim)?;
        Ok(())
    }

    /// Validate input timing against server state
    pub fn validate_timing(
        &self,
        input_tick: u64,
        server_tick: u64,
        rtt_ticks: u64,
    ) -> Result<(), CheatViolation> {
        // Check for future inputs (client claiming tick ahead of server)
        if input_tick > server_tick + self.config.max_future_ticks {
            return Err(CheatViolation::FutureInput(input_tick, server_tick));
        }

        // Check for stale inputs (account for RTT)
        let max_delay = rtt_ticks + self.config.max_stale_ticks;
        if server_tick > input_tick + max_delay {
            return Err(CheatViolation::StaleInput(input_tick, server_tick, max_delay));
        }

        Ok(())
    }

    /// Validate sequence number progression
    pub fn validate_sequence(
        &self,
        prev_sequence: u64,
        current_sequence: u64,
    ) -> Result<(), CheatViolation> {
        // Check for regression
        if current_sequence < prev_sequence {
            return Err(CheatViolation::SequenceRegression(
                prev_sequence,
                current_sequence,
            ));
        }

        // Check for suspicious jumps
        let jump = current_sequence - prev_sequence;
        if jump > self.config.max_sequence_jump {
            return Err(CheatViolation::SequenceJump(prev_sequence, current_sequence));
        }

        Ok(())
    }

    /// Validate that all float values are valid (not NaN or Infinity)
    fn validate_vector_values(&self, input: &PlayerInput) -> Result<(), CheatViolation> {
        if !input.thrust.x.is_finite()
            || !input.thrust.y.is_finite()
            || !input.aim.x.is_finite()
            || !input.aim.y.is_finite()
        {
            return Err(CheatViolation::InvalidFloats);
        }
        Ok(())
    }

    /// Validate thrust vector magnitude
    fn validate_thrust(&self, thrust: &Vec2) -> Result<(), CheatViolation> {
        let magnitude = thrust.magnitude();
        if magnitude > self.config.max_vector_magnitude {
            return Err(CheatViolation::InvalidThrust(magnitude));
        }
        Ok(())
    }

    /// Validate aim vector magnitude
    fn validate_aim(&self, aim: &Vec2) -> Result<(), CheatViolation> {
        let magnitude = aim.magnitude();
        // Aim can be zero (not aiming) or normalized
        if magnitude > self.config.max_vector_magnitude {
            return Err(CheatViolation::InvalidAim(magnitude));
        }
        Ok(())
    }
}

impl Default for InputValidator {
    fn default() -> Self {
        Self::new(ValidationConfig::default())
    }
}

/// Sanitize input by clamping values to valid ranges
/// Use this after validation to ensure safe processing
pub fn sanitize_input(input: &mut PlayerInput) {
    // Clamp thrust to unit length
    if input.thrust.magnitude() > 1.0 {
        input.thrust = input.thrust.normalize();
    }

    // Clamp aim to unit length
    if input.aim.magnitude() > 1.0 {
        input.aim = input.aim.normalize();
    }

    // Replace any NaN/Infinity with zero
    if !input.thrust.x.is_finite() || !input.thrust.y.is_finite() {
        input.thrust = Vec2::ZERO;
    }
    if !input.aim.x.is_finite() || !input.aim.y.is_finite() {
        input.aim = Vec2::ZERO;
    }
}

/// Quick validation function for common use
pub fn validate_input(input: &PlayerInput) -> Result<(), CheatViolation> {
    InputValidator::default().validate_input(input)
}

/// Quick timing validation
pub fn validate_timing(
    input_tick: u64,
    server_tick: u64,
    rtt_ms: u32,
) -> Result<(), CheatViolation> {
    let rtt_ticks = (rtt_ms as u64) / 33; // 33ms per tick at 30Hz
    InputValidator::default().validate_timing(input_tick, server_tick, rtt_ticks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> PlayerInput {
        PlayerInput {
            sequence: 1,
            tick: 100,
            thrust: Vec2::new(0.5, 0.5),
            aim: Vec2::new(1.0, 0.0),
            boost: false,
            fire: false,
            fire_released: false,
        }
    }

    #[test]
    fn test_valid_input() {
        let validator = InputValidator::default();
        let input = valid_input();
        assert!(validator.validate_input(&input).is_ok());
    }

    #[test]
    fn test_invalid_thrust_magnitude() {
        let validator = InputValidator::default();
        let mut input = valid_input();
        input.thrust = Vec2::new(2.0, 0.0);

        let result = validator.validate_input(&input);
        assert!(matches!(result, Err(CheatViolation::InvalidThrust(_))));
    }

    #[test]
    fn test_invalid_aim_magnitude() {
        let validator = InputValidator::default();
        let mut input = valid_input();
        input.aim = Vec2::new(1.5, 1.5);

        let result = validator.validate_input(&input);
        assert!(matches!(result, Err(CheatViolation::InvalidAim(_))));
    }

    #[test]
    fn test_nan_thrust() {
        let validator = InputValidator::default();
        let mut input = valid_input();
        input.thrust = Vec2::new(f32::NAN, 0.0);

        let result = validator.validate_input(&input);
        assert!(matches!(result, Err(CheatViolation::InvalidFloats)));
    }

    #[test]
    fn test_infinity_aim() {
        let validator = InputValidator::default();
        let mut input = valid_input();
        input.aim = Vec2::new(f32::INFINITY, 0.0);

        let result = validator.validate_input(&input);
        assert!(matches!(result, Err(CheatViolation::InvalidFloats)));
    }

    #[test]
    fn test_zero_vectors_valid() {
        let validator = InputValidator::default();
        let mut input = valid_input();
        input.thrust = Vec2::ZERO;
        input.aim = Vec2::ZERO;

        assert!(validator.validate_input(&input).is_ok());
    }

    #[test]
    fn test_timing_valid() {
        let validator = InputValidator::default();
        let result = validator.validate_timing(100, 100, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_timing_future_input() {
        let validator = InputValidator::default();
        // Client claims tick 110 when server is at 100
        let result = validator.validate_timing(110, 100, 0);
        assert!(matches!(result, Err(CheatViolation::FutureInput(_, _))));
    }

    #[test]
    fn test_timing_slightly_future_ok() {
        let validator = InputValidator::default();
        // Client 2 ticks ahead is allowed
        let result = validator.validate_timing(102, 100, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_timing_stale_input() {
        let validator = InputValidator::default();
        // Client claims tick 50 when server is at 100 with 0 RTT
        let result = validator.validate_timing(50, 100, 0);
        assert!(matches!(result, Err(CheatViolation::StaleInput(_, _, _))));
    }

    #[test]
    fn test_timing_with_rtt_compensation() {
        let validator = InputValidator::default();
        // With high RTT, older inputs should be accepted
        // RTT of 10 ticks + 30 max stale = 40 tick window
        let result = validator.validate_timing(65, 100, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sequence_valid_increment() {
        let validator = InputValidator::default();
        let result = validator.validate_sequence(10, 11);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sequence_same_ok() {
        let validator = InputValidator::default();
        // Resending same sequence (e.g., retry) is ok
        let result = validator.validate_sequence(10, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_sequence_regression() {
        let validator = InputValidator::default();
        let result = validator.validate_sequence(10, 5);
        assert!(matches!(
            result,
            Err(CheatViolation::SequenceRegression(_, _))
        ));
    }

    #[test]
    fn test_sequence_jump() {
        let validator = InputValidator::default();
        // Jump of 200 exceeds max of 100
        let result = validator.validate_sequence(10, 210);
        assert!(matches!(result, Err(CheatViolation::SequenceJump(_, _))));
    }

    #[test]
    fn test_sanitize_input_clamps_thrust() {
        let mut input = valid_input();
        input.thrust = Vec2::new(10.0, 0.0);

        sanitize_input(&mut input);

        assert!((input.thrust.magnitude() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_sanitize_input_clamps_aim() {
        let mut input = valid_input();
        input.aim = Vec2::new(5.0, 5.0);

        sanitize_input(&mut input);

        assert!((input.aim.magnitude() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_sanitize_input_fixes_nan() {
        let mut input = valid_input();
        input.thrust = Vec2::new(f32::NAN, f32::NAN);

        sanitize_input(&mut input);

        assert_eq!(input.thrust, Vec2::ZERO);
    }

    #[test]
    fn test_sanitize_preserves_valid() {
        let mut input = valid_input();
        let original_thrust = input.thrust;

        sanitize_input(&mut input);

        assert_eq!(input.thrust, original_thrust);
    }

    #[test]
    fn test_quick_validate_function() {
        let input = valid_input();
        assert!(validate_input(&input).is_ok());

        let mut bad_input = valid_input();
        bad_input.thrust = Vec2::new(100.0, 0.0);
        assert!(validate_input(&bad_input).is_err());
    }

    #[test]
    fn test_quick_timing_function() {
        assert!(validate_timing(100, 100, 50).is_ok());
        assert!(validate_timing(200, 100, 50).is_err());
    }

    #[test]
    fn test_custom_config() {
        let config = ValidationConfig {
            max_vector_magnitude: 0.5, // Stricter than default
            ..Default::default()
        };
        let validator = InputValidator::new(config);

        let mut input = valid_input();
        input.thrust = Vec2::new(0.6, 0.0); // Would pass default, fails here

        assert!(validator.validate_input(&input).is_err());
    }
}
