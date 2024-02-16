use std::fs;
use std::path::Path;
use std::process::Command;

pub mod conf;
pub use conf::Conf;

pub mod cli;
use cli::Args;

pub mod build;

mod download;

fn find_package(conf: &Conf, name: &str) -> Option<String> {
    let paths = fs::read_dir(conf.pkg_dir(name)).unwrap();
    for path in paths {
        let path = path.unwrap().file_name().into_string().unwrap();
        if path.ends_with(".pkg.tar.zst") {
            return Some(path);
        }
    }
    None
}

// aerc-0.16.0-1-x86_64.pkg.tar.zst
fn repo_add(conf: &Conf) {
    let db = Path::new(&conf.server_dir).join("archage.db.tar.gz");
    for pkg in &conf.packages {
        if let Some(package_file) = find_package(conf, &pkg) {
            println!("repo-add -> {}", pkg);
            // Move the package next to the db
            let moved_package_file = Path::new(&conf.server_dir).join(&package_file);
            std::fs::rename(
                Path::new(&conf.server_dir).join(pkg).join(&package_file),
                &moved_package_file,
            )
            .unwrap();
            let output = Command::new("repo-add")
                .current_dir(&conf.server_dir)
                .args([&db, &moved_package_file])
                .output()
                .unwrap();
            if !output.status.success() {
                eprintln!(
                    "Failed to add {} to the db:\n stdout: {}\nstderr:{}",
                    pkg,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        } else {
            eprintln!("Failed to find {} package file", pkg);
        }
    }
}

fn main() {
    let args = Args::get();
    let conf = Conf::new(args.conf.as_deref());

    conf.print();

    fs::create_dir_all(&conf.server_dir).unwrap();
    if !args.skip_download {
        println!("Downloading packages...");
        download::download_all(&conf);
    }
    println!("Building packages...");
    build::build_all(&conf);
    println!("Adding packages...");
    repo_add(&conf);
}
