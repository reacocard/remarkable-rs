use std::fs;
use std::path;

use directories::ProjectDirs;

use remarkable_cloud_api::*;

fn print_documents(docs: &Documents, path: &Option<&path::Path>, recurse: bool, prefix: &str) {
    let doc_id = path
        .and_then(|v| docs.get_by_path(v))
        .and_then(|v| Some(v.id));
    for doc in docs.get_children(&doc_id) {
        println!("{}{} {}", prefix, doc.visible_name, doc.id);
        if recurse {
            let p = path.map_or_else(
                || path::PathBuf::from(&doc.visible_name),
                |p| p.join(&doc.visible_name),
            );
            print_documents(&docs, &Some(p.as_path()), recurse, &format!("{}  ", prefix));
        }
    }
}

async fn get_client(state_path: &path::Path) -> Result<Client, RemarkableError> {
    let mut client = Client::new(
        ClientState::new(),
        reqwest::Client::builder()
            .user_agent("remarkable-cloud")
            .build()?,
    );
    client.state().load_from_path(state_path)?;
    client.refresh_token().await?;
    Ok(client)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = clap::App::new("reMarkable cloud cli")
        .subcommand(
            clap::SubCommand::with_name("ls")
                .arg(clap::Arg::with_name("recurse").short("r"))
                .arg(clap::Arg::with_name("path").index(1)),
        )
        .subcommand(
            clap::SubCommand::with_name("info")
                .arg(clap::Arg::with_name("filename").index(1).required(true)),
        )
        .get_matches();

    let project_dirs = match ProjectDirs::from("zone", "ounce", "remarkable-cloud") {
        Some(x) => x,
        None => panic!("Could not determine settings directory."),
    };
    let config_dir = project_dirs.config_dir();
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }
    let client_state_path = config_dir.join("client_state.json");

    let client = get_client(&client_state_path).await?;

    match matches.subcommand() {
        ("ls", Some(sub_m)) => {
            let documents = client.get_documents().await?;
            print_documents(
                &documents,
                &sub_m.value_of("path").map(path::Path::new),
                sub_m.is_present("recurse"),
                "",
            );
        }
        ("info", Some(sub_m)) => {
            let documents = client.get_documents().await?;
            let document =
                documents.get_by_path(&path::Path::new(sub_m.value_of("filename").unwrap()));
            println!("{:?}", document);
        }
        _ => panic!("Subcommand not found."),
    }
    Ok(())
}
