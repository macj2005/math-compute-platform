pub fn estimate_pi(iterations: u64) -> f64 {
    if iterations == 0 {
        return 0.0;
    }

    let mut inside_circle = 0;
    for _ in 0..iterations {
        let x = rand::random::<f64>();
        let y = rand::random::<f64>();

        if (x * x) + (y * y) <= 1.0 {
            inside_circle += 1;
        }
    }

    4.0 * inside_circle as f64 / iterations as f64
}
