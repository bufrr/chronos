pub mod message;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use crate::message::ZMessage;
    use super::*;

    #[test]
    fn send_message() {
        let mut msg = ZMessage::default();
        msg.data = vec![1];
    }
}