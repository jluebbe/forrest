mod auth;
mod config;
mod ingres;
mod jobs;
mod machines;

async fn forrest() -> anyhow::Result<()> {
    let config_path = {
        let mut args: Vec<String> = std::env::args().collect();

        match args.len() {
            1 => "config.yaml".to_owned(),
            2 => args.remove(1),
            _ => anyhow::bail!("Usage: {} [CONFIG]", args[0]),
        }
    };

    // Read the config file.
    // The file will be re-read if it changed on disk at many points in the program,
    // allowing changes to be made while jobs are being executed.
    let config = config::Config::new(&config_path)?;

    // We use a private key to authenticate as a GitHub application
    // and derive installation tokens from it.
    // Use a central registry of cached installation tokens for efficiency.
    let auth = auth::Auth::new(&config)?;

    // The job manager keeps track of build jobs and their status and
    // communicates the demand for machines with the machine manager.
    // It gets its updates from from the webhook handler and poller below.
    let job_manager = jobs::Manager::new();

    // The main method to learn about new jobs to run is via webhooks.
    // These are POST requests sent by GitHub notifying us about events.
    let mut webhook =
        ingres::WebhookHandler::new(config.clone(), auth.clone(), job_manager.clone())?;

    // Our secondary source of information are periodic polls of the GitHub API.
    // These come in handy at startup or after network outages when we may have
    // missed webhooks.
    let poller = ingres::Poller::new(config.clone(), auth.clone(), job_manager);

    // Make sure we can reach GitHub and our authentication works before
    // signaling readiness to systemd.
    poller.poll_once().await?;

    log::info!("Startup complete. Handling requests");

    // Notify systemd that we are ready to handle requests.
    // This allows us to use the `Type=notify` systemd service type.
    if let Err(e) = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]) {
        log::info!("Failed to notify systemd about service startup: {e}");
    }

    tokio::select! {
        res = webhook.run() => res,
        res = poller.poll() => res,
    }?;

    Ok(())
}

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init();

    // Run in a single-threaded async runtime.
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?
        .block_on(forrest())
}