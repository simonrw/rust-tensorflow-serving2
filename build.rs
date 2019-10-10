use std::path::PathBuf;
// use walkdir::WalkDir;

fn main() {
    // let protos: Vec<_> = WalkDir::new("protos")
    //     .into_iter()
    //     .filter_map(Result::ok)
    //     .map(|e| e.into_path())
    //     .filter(|p| {
    //         // Get only the files that are proto files
    //         let ext = p.extension();
    //         match ext {
    //             Some(ext) => ext == "proto",
    //             None => false,
    //         }
    //     })
    //     .collect();
    let protos = &[
        // "protos/tensorflow_serving/apis/classification.proto",
        "protos/tensorflow_serving/apis/prediction_service.proto",
    ]
    .iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();

    match tonic_build::configure().compile(&protos, &[PathBuf::from("protos")]) {
        Ok(_) => {}
        Err(e) => {
            panic!("{}", e);
        }
    }
}
