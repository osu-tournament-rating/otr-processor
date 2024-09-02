use indicatif::ProgressBar;

pub fn progress_bar(len: u64, msg: String) -> Option<ProgressBar> {
    if cfg!(test) {
        return None;
    }

    let bar = ProgressBar::new(len).with_message(msg);
    bar.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise} / {eta_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
            .unwrap()
            .progress_chars("##-")
    );

    Some(bar)
}

pub fn progress_bar_spinner(len: u64, msg: String) -> Option<ProgressBar> {
    if cfg!(test) {
        return None;
    }

    let bar = ProgressBar::new(len).with_message(msg);
    bar.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("[{elapsed_precise} / {eta_precise}] {spinner:.green} {msg}")
            .unwrap()
    );

    Some(bar)
}

pub fn indeterminate_bar(msg: String) -> Option<ProgressBar> {
    if cfg!(test) {
        return None;
    }

    let bar = ProgressBar::new_spinner().with_message(msg);

    bar.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("[{elapsed_precise}] {spinner:.green} {msg}")
            .unwrap()
    );

    Some(bar)
}
