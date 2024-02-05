use indicatif::ProgressBar;

pub fn progress_bar(len: u64) -> ProgressBar {
    let bar = ProgressBar::new(len);
    bar.set_style(indicatif::ProgressStyle::default_bar()
        .template("[{elapsed_precise} / {eta_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .unwrap().progress_chars("##-"));

    bar
}