//! Validated runtime aircraft attitude.

use nalgebra::{Matrix3, Quaternion, Rotation3, UnitQuaternion, Vector3};
use thiserror::Error;

use crate::pb;

const ROTATION_TOLERANCE: f64 = 1.0e-6;

/// Error returned when an attitude representation is not a valid SO(3) value.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum AttitudeError {
    /// A quaternion contains a non-finite component or has zero norm.
    #[error("quaternion must contain only finite values and have non-zero norm")]
    InvalidQuaternion,
    /// Euler angles contain a non-finite component.
    #[error("Euler angles must contain only finite values")]
    InvalidEuler,
    /// A rotation matrix contains non-finite values or is not a proper rotation.
    #[error("rotation matrix must be finite, orthogonal, and have determinant +1")]
    InvalidRotationMatrix,
}

/// A validated BODY-FRD to local-NED attitude.
///
/// The canonical external quaternion convention is Hamilton scalar-first
/// `[w, x, y, z]`. Rotation matrices use row-major storage.
#[derive(Debug, Clone, PartialEq)]
pub struct Attitude(UnitQuaternion<f64>);

impl Attitude {
    /// Return the identity attitude.
    pub fn identity() -> Self {
        Self(UnitQuaternion::identity())
    }

    /// Construct from a Hamilton quaternion in scalar-first `[w, x, y, z]` order.
    pub fn from_quaternion(value: [f64; 4]) -> Result<Self, AttitudeError> {
        if !value.iter().all(|component| component.is_finite()) {
            return Err(AttitudeError::InvalidQuaternion);
        }
        let quaternion = Quaternion::new(value[0], value[1], value[2], value[3]);
        let norm = quaternion.norm();
        if norm <= f64::EPSILON {
            return Err(AttitudeError::InvalidQuaternion);
        }
        Ok(Self(UnitQuaternion::new_normalize(quaternion)))
    }

    /// Construct from a BODY-FRD to local-NED 3x3 rotation matrix stored row-major.
    pub fn from_rotation_matrix(value: [f64; 9]) -> Result<Self, AttitudeError> {
        if !value.iter().all(|component| component.is_finite()) {
            return Err(AttitudeError::InvalidRotationMatrix);
        }
        let matrix = Matrix3::from_row_slice(&value);
        let orthogonality_error = (matrix.transpose() * matrix - Matrix3::identity()).norm();
        if orthogonality_error > ROTATION_TOLERANCE
            || (matrix.determinant() - 1.0).abs() > ROTATION_TOLERANCE
        {
            return Err(AttitudeError::InvalidRotationMatrix);
        }
        Ok(Self(UnitQuaternion::from_rotation_matrix(
            &Rotation3::from_matrix_unchecked(matrix),
        )))
    }

    /// Construct from Z-Y-X aerospace `[roll, pitch, yaw]` Euler angles in radians.
    pub fn from_euler(value: [f64; 3]) -> Result<Self, AttitudeError> {
        if !value.iter().all(|component| component.is_finite()) {
            return Err(AttitudeError::InvalidEuler);
        }
        Ok(Self(UnitQuaternion::from_euler_angles(
            value[0], value[1], value[2],
        )))
    }

    /// Return the Hamilton quaternion in scalar-first `[w, x, y, z]` order.
    pub fn quaternion(&self) -> [f64; 4] {
        let quaternion = self.0.quaternion();
        [quaternion.w, quaternion.i, quaternion.j, quaternion.k]
    }

    /// Return the BODY-FRD to local-NED 3x3 rotation matrix in row-major order.
    pub fn rotation_matrix(&self) -> [f64; 9] {
        let matrix = self.0.to_rotation_matrix();
        let matrix = matrix.matrix();
        [
            matrix[(0, 0)],
            matrix[(0, 1)],
            matrix[(0, 2)],
            matrix[(1, 0)],
            matrix[(1, 1)],
            matrix[(1, 2)],
            matrix[(2, 0)],
            matrix[(2, 1)],
            matrix[(2, 2)],
        ]
    }

    /// Return Z-Y-X aerospace `[roll, pitch, yaw]` Euler angles in radians.
    pub fn euler(&self) -> [f64; 3] {
        let (roll, pitch, yaw) = self.0.euler_angles();
        [roll, pitch, yaw]
    }

    /// Rotate a BODY-FRD vector into local NED.
    pub fn rotate_body_to_ned(&self, value: [f64; 3]) -> [f64; 3] {
        let result = self.0.transform_vector(&Vector3::from(value));
        result.into()
    }

    /// Rotate a local-NED vector into BODY-FRD.
    pub fn rotate_ned_to_body(&self, value: [f64; 3]) -> [f64; 3] {
        let result = self.0.inverse_transform_vector(&Vector3::from(value));
        result.into()
    }

    /// Interpolate between two attitudes along the shortest SO(3) arc.
    pub fn slerp(&self, other: &Self, factor: f64) -> Self {
        Self(self.0.slerp(&other.0, factor))
    }
}

impl Default for Attitude {
    fn default() -> Self {
        Self::identity()
    }
}

impl TryFrom<&pb::Quaternion> for Attitude {
    type Error = AttitudeError;

    fn try_from(value: &pb::Quaternion) -> Result<Self, Self::Error> {
        Self::from_quaternion([value.w, value.x, value.y, value.z])
    }
}

impl From<&Attitude> for pb::Quaternion {
    fn from(value: &Attitude) -> Self {
        let [w, x, y, z] = value.quaternion();
        Self { w, x, y, z }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use approx::assert_relative_eq;

    use super::*;

    #[test]
    fn representations_describe_the_same_rotation() {
        let euler = Attitude::from_euler([0.0, 0.0, FRAC_PI_2]).unwrap();
        let quaternion = Attitude::from_quaternion(euler.quaternion()).unwrap();
        let matrix = Attitude::from_rotation_matrix(euler.rotation_matrix()).unwrap();
        for attitude in [quaternion, matrix] {
            assert_relative_eq!(attitude.euler()[2], FRAC_PI_2, epsilon = 1.0e-12);
            assert_relative_eq!(
                attitude.rotate_body_to_ned([1.0, 0.0, 0.0])[1],
                1.0,
                epsilon = 1.0e-12
            );
        }
    }

    #[test]
    fn rejects_invalid_representations() {
        assert_eq!(
            Attitude::from_quaternion([0.0; 4]),
            Err(AttitudeError::InvalidQuaternion)
        );
        assert_eq!(
            Attitude::from_rotation_matrix([0.0; 9]),
            Err(AttitudeError::InvalidRotationMatrix)
        );
        assert_eq!(
            Attitude::from_rotation_matrix([-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,]),
            Err(AttitudeError::InvalidRotationMatrix)
        );
    }

    #[test]
    fn protobuf_round_trip_preserves_attitude() {
        let attitude = Attitude::from_euler([0.2, -0.3, 0.4]).unwrap();
        let encoded = pb::Quaternion::from(&attitude);
        let decoded = Attitude::try_from(&encoded).unwrap();
        for (actual, expected) in decoded
            .rotation_matrix()
            .iter()
            .zip(attitude.rotation_matrix())
        {
            assert_relative_eq!(*actual, expected, epsilon = 1.0e-12);
        }
    }
}
