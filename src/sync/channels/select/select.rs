// TODO

#[cfg(test)]
mod tests {
    use crate as orengine;
    use crate::sync::{AsyncChannel, LocalChannel};
    use orengine_macros::select;

    #[orengine::test::test_local]
    fn test_select() {
        use crate as orengine;

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        let a = select! {
            recv(&ch2) -> res => { res; 1 }
            recv(&ch1) -> res => { res; 4 }
            send(&ch3, 20) -> res => {
                if let Err(e) = res {
                    2
                } else {
                    3
                }
            }
            default => { 5 }
        };

        let ch1 = LocalChannel::<u32>::bounded(1);
        let ch2 = LocalChannel::<u32>::bounded(1);
        let ch3 = LocalChannel::<u32>::bounded(1);

        let b = select! {
            recv(&ch2) -> res => { res; 1 }
            recv(&ch1) -> res => { res; 2 }
            send(&ch3, 20) -> res => { res; 3 }
        };
    }
}
