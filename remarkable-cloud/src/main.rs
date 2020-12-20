use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use zip::ZipArchive;

use remarkable_cloud_api::*;

fn print_documents(
    docs: &Documents,
    path: &Option<&Path>,
    recurse: bool,
    prefix: &str,
) {
    let doc_id = match path {
        None => None,
        Some(p) => match p.to_string_lossy().into_owned().as_str() {
            "/" => None,
            _ => match docs.get_by_path(p) {
                None => {
                    println!("Couldn't find {:?}", p);
                    return;
                }
                Some(d) => Some(d.id),
            },
        },
    };
    for doc in docs.get_children(&doc_id) {
        println!("{}{} {}", prefix, doc.visible_name, doc.id);
        if recurse {
            let p = path.map_or_else(
                || PathBuf::from(&doc.visible_name),
                |p| p.join(&doc.visible_name),
            );
            print_documents(
                &docs,
                &Some(p.as_path()),
                recurse,
                &format!("{}  ", prefix),
            );
        }
    }
}

fn add_ext_to_path(path: &Path, ext: &str) -> PathBuf {
    let mut buf = path.to_path_buf();
    let mut newext = path.extension().unwrap_or_default().to_os_string();
    if newext.len() > 0 {
        newext.push(".");
    }
    newext.push(ext);
    buf.set_extension(newext);
    buf
}

fn paths_from_arg<'a>(
    matches: &'a clap::ArgMatches,
    arg_name: &str,
) -> Box<dyn Iterator<Item = &'a Path> + 'a> {
    paths_from_arg_or(matches, arg_name, None)
}

fn paths_from_arg_or<'a>(
    matches: &'a clap::ArgMatches,
    arg_name: &str,
    default: Option<&'a Path>,
) -> Box<dyn Iterator<Item = &'a Path> + 'a> {
    match matches.values_of(arg_name) {
        Some(i) => Box::new(i.map(Path::new)),
        None => match default {
            Some(d) => Box::new(std::iter::once(d)),
            None => Box::new(std::iter::empty()),
        },
    }
}

async fn get_client(state_path: &Path) -> Result<Client> {
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
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let matches = clap::App::new("reMarkable cloud cli")
        .subcommand(
            clap::SubCommand::with_name("ls")
                .about("Lists files.")
                .arg(clap::Arg::with_name("recurse")
                     .short("r")
                     .long("recursive")
                     .help("Lists files recursively"))
                // TODO: accept multiple paths
                .arg(clap::Arg::with_name("paths")
                     .index(1)
                     .multiple(true)),
        )
        .subcommand(
            clap::SubCommand::with_name("info")
                .about("Describes a file in detail.")
                // TODO: accept multiple files
                .arg(clap::Arg::with_name("filenames")
                     .index(1)
                     .multiple(true)
                     .required(true)),
        )
        .subcommand(
            clap::SubCommand::with_name("pull")
                .about("Downloads files.")
                .arg(clap::Arg::with_name("raw-zip")
                     .long("raw-zip")
                     .hidden(true)
                     .help("Gets the raw .zip from the API rather than extracting the document. Mostly useful for development."))
                .setting(clap::AppSettings::TrailingVarArg)
                .arg(clap::Arg::with_name("filenames")
                     .index(1)
                     .multiple(true)
                     .required(true)),
        )
        .get_matches();

    let project_dirs =
        match ProjectDirs::from("zone", "ounce", "remarkable-cloud") {
            Some(x) => x,
            None => panic!("Could not determine settings directory."),
        };
    let config_dir = project_dirs.config_dir();
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }
    let client_state_path = config_dir.join("client_state.json");

    match matches.subcommand() {
        ("ls", Some(sub_m)) => {
            let client = get_client(&client_state_path).await?;
            let documents = client.get_documents().await?;
            for path in paths_from_arg_or(sub_m, "paths", Some(Path::new("/")))
            {
                print_documents(
                    &documents,
                    &Some(&path),
                    sub_m.is_present("recurse"),
                    "",
                );
            }
        }
        ("info", Some(sub_m)) => {
            let client = get_client(&client_state_path).await?;
            let documents = client.get_documents().await?;
            for filepath in paths_from_arg(sub_m, "filenames") {
                match documents.get_by_path(&filepath) {
                    Some(d) => println!("{:?}", d),
                    None => println!("Couldn't find document '{:?}'", filepath),
                }
            }
        }
        ("pull", Some(sub_m)) => {
            let client = get_client(&client_state_path).await?;
            let documents = client.get_documents().await?;
            for filepath in paths_from_arg(sub_m, "filenames") {
                let docbytes = match documents.get_by_path(&filepath) {
                    None => {
                        println!("Couldn't find document '{:?}'", filepath);
                        continue;
                    }
                    Some(doc) => {
                        let blobdoc =
                            client.get_document_by_id(&doc.id).await?;
                        //println!("{:?}", blobdoc);
                        // TODO: add progress indicator
                        client
                            .http()
                            .get(&blobdoc.blob_url_get)
                            .send()
                            .await?
                            .bytes()
                            .await?
                    }
                };
                match sub_m.is_present("raw-zip") {
                    true => {
                        let fp = add_ext_to_path(filepath, "zip");
                        fs::write(fp, docbytes)?;
                    }
                    false => {
                        let mut za =
                            ZipArchive::new(std::io::Cursor::new(docbytes))?;
                        let f = match za.file_names().find(|i| {
                            i.ends_with(".pdf") || i.ends_with(".epub")
                        }) {
                            Some(f) => f,
                            None => {
                                println!(
                                    "No file found in response for {:?}",
                                    filepath
                                );
                                continue;
                            }
                        }
                        .to_string();
                        let ext = Path::new(&f)
                            .extension()
                            .unwrap_or_default()
                            .to_string_lossy();
                        let fp = add_ext_to_path(filepath, &ext);
                        println!("DEBUG: {:?}", fp);
                        // TODO: Handle overwriting
                        match fp.file_name() {
                            Some(fpn) => { std::io::copy(
                                &mut za.by_name(&f)?,
                                &mut fs::File::create(fpn)?,
                            )?; },
                            None => println!("No filename found in path {:?}", fp),
                        }
                    }
                }
            }
        }
        _ => panic!("Subcommand not found."),
    }
    Ok(())
}
