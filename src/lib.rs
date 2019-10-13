extern crate prost_types;

use image::{DynamicImage, GenericImageView};
use std::collections::HashMap;
use std::error::Error;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

/// Our custom result type
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Trait representing either an image or some image data
///
pub trait Image {
    /// Extract an image from the enclosed object
    ///
    fn to_image(&self) -> Result<DynamicImage>;
}

impl Image for dyn AsRef<Path> {
    fn to_image(&self) -> Result<DynamicImage> {
        image::open(self).map_err(From::from)
    }
}

impl Image for DynamicImage {
    fn to_image(&self) -> Result<DynamicImage> {
        Ok(self.clone())
    }
}

impl Image for PathBuf {
    fn to_image(&self) -> Result<DynamicImage> {
        image::open(self).map_err(From::from)
    }
}

impl Image for &str {
    fn to_image(&self) -> Result<DynamicImage> {
        image::open(self).map_err(From::from)
    }
}

pub(crate) mod tensorflow {
    tonic::include_proto!("tensorflow");
    pub(crate) mod tensorflow_serving {
        tonic::include_proto!("tensorflow.serving");
    }
}

use tensorflow::tensorflow_serving::{
    client::PredictionServiceClient, input, model_spec::VersionChoice, ClassificationRequest,
    ClassificationResult, ExampleList, Input, ModelSpec, PredictRequest, PredictResponse,
};
use tensorflow::{
    feature, feature::Kind, tensor_shape_proto, BytesList, DataType, Example, Feature, Features,
    FloatList, Int64List, TensorProto, TensorShapeProto,
};

/// Builder pattern used to build the client.
///
/// This struct is created by calling `TensorflowServing::new()`. It represents a partially
/// configured client. Use the builder pattern to construct a client gradually. Then call the
/// `build` method to construct a concrete `TensorflowServing` instance.
///
/// Required parameters are:
///
/// * hostname
/// * port
/// * model_name
///
/// `signature_name` is optional, and defaults to "serving_default".
///
#[derive(Default)]
pub struct TensorflowServingBuilder {
    hostname: Option<String>,
    port: Option<u16>,
    signature_name: Option<String>,
}

impl TensorflowServingBuilder {
    /// Set the hostname for the client
    ///
    pub fn hostname<S: Into<String>>(&mut self, hostname: S) -> &mut Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Set the port for the client
    ///
    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = Some(port);
        self
    }

    /// Set the signature name
    ///
    pub fn signature_name<S: Into<String>>(&mut self, signature_name: S) -> &mut Self {
        self.signature_name = Some(signature_name.into());
        self
    }

    /// Build a `TensorflowServing` client.
    ///
    pub fn build(&mut self) -> Result<TensorflowServing> {
        if let None = self.hostname {
            return Err(format!("hostname not provided").into());
        }

        if let None = self.port {
            return Err(format!("port not provided").into());
        }

        let signature_name = self
            .signature_name
            .take()
            .unwrap_or_else(|| "serving_default".to_string());

        let hostname = self.hostname.take().unwrap();
        let client = PredictionServiceClient::connect(format!(
            "http://{}:{}",
            hostname,
            self.port.unwrap()
        ))?;

        Ok(TensorflowServing {
            client,
            signature_name,
        })
    }
}

/// Tensorflow Serving client
///
/// Used to talk to a Tensorflow Serving server.
///
pub struct TensorflowServing {
    client: PredictionServiceClient<tonic::transport::Channel>,
    signature_name: String,
}

impl TensorflowServing {
    /// Construct a new `TensorflowServing` builder struct.
    ///
    pub async fn new() -> TensorflowServingBuilder {
        TensorflowServingBuilder::default()
    }

    /// Run a classification on a supplied image
    ///
    pub async fn classify<S, T, F, V>(
        &mut self,
        model_name: S,
        payload_map: HashMap<T, V>,
    ) -> Result<ClassificationResult>
    where
        S: Into<ModelDescription<F>>,
        F: Into<String>,
        T: Into<String>,
        V: Into<Payload>,
    {
        let req = ClassificationRequest {
            model_spec: Some(self.build_model_spec(model_name)).into(),
            input: Some(self.build_input(payload_map)).into(),
            ..Default::default()
        };

        let request = tonic::Request::new(req);
        let resp = self.client.classify(request).await?;
        unimplemented!("{:#?}", resp)

        /*
        let resp = self.prediction_client.classify(&req)?;
        // TODO: remove this unwrap
        Ok(resp.result.unwrap())
        */
    }

    /// Run a prediction for a supplied image
    ///
    /// Supply something that implements `Into<Image>` i.e. either a path to an image file, or
    /// an already open `image::DynamicImage`, and a [`ModelDescription`][model-description],
    /// to get a prediction from the server.
    ///
    /// The `preprocessing_fn` parameter allows customisation of the pixel values.
    ///
    /// [model-description]: struct.ModelDescription.html
    pub async fn predict_with_preprocessing<I, F, S, M>(
        &mut self,
        img: I,
        model_description: S,
        preprocessing_fn: M,
    ) -> Result<PredictionResult>
    where
        I: Image,
        F: Into<String>,
        S: Into<ModelDescription<F>>,
        M: Fn(f32) -> f32,
    {
        // Load data
        let img = img.to_image()?;

        let (width, height) = img.dimensions();
        let dims: Vec<_> = [1, width as i64, height as i64, 3]
            .iter()
            .map(|d| tensor_shape_proto::Dim {
                size: *d,
                name: "".to_string(),
            })
            .collect();

        let pixels: Vec<_> = img
            .raw_pixels()
            .iter()
            .map(|p| *p as f32)
            .map(preprocessing_fn)
            .collect();

        let tensor_shape = TensorShapeProto {
            dim: dims.into(),
            ..Default::default()
        };

        let tensor = TensorProto {
            dtype: 1,
            tensor_shape: Some(tensor_shape).into(),
            float_val: pixels,
            ..Default::default()
        };

        let mut inputs = HashMap::new();
        inputs.insert("input".into(), tensor);

        let request = PredictRequest {
            model_spec: Some(self.build_model_spec(model_description)).into(),
            inputs,
            ..Default::default()
        };

        let resp = self.client.predict(tonic::Request::new(request)).await?;
        unimplemented!("{:#?}", resp);
        // PredictionResult::from_raw(resp)
    }

    /*
    /// Run a regression job
    pub fn regress<S, T, F, V>(
        &self,
        model_name: S,
        payload_map: HashMap<T, V>,
    ) -> Result<regression::RegressionResult>
    where
        S: Into<ModelDescription<F>>,
        F: Into<String>,
        T: Into<String>,
        V: Into<Payload>,
    {
        let req = regression::RegressionRequest {
            model_spec: Some(self.build_model_spec(model_name)).into(),
            input: Some(self.build_input(payload_map)).into(),
            ..Default::default()
        };

        let resp = self.prediction_client.regress(&req)?;
        Ok(resp.result.unwrap())
    }

    /// Run a prediction (see [predict-with-preprocessing](struct.TensorflowServing.html#method.predict_with_preprocessing))
    pub fn predict<I, F, S>(&self, img: I, model_description: S) -> Result<PredictionResult>
    where
        I: Image,
        F: Into<String>,
        S: Into<ModelDescription<F>>,
    {
        self.predict_with_preprocessing(img, model_description, |p| p)
    }

    /// Perform multi-inference
    pub fn multi_inference<S, V>(
        &self,
        _model_name: S,
        _payload_map: HashMap<S, V>,
    ) -> Result<inference::MultiInferenceResponse>
    where
        S: Into<String>,
        V: Into<Payload>,
    {
        unimplemented!();
        /*
        let request = inference::MultiInferenceRequest {
            // TODO: tasks
            input: Some(self.build_input(payload_map)).into(),
            ..Default::default()
        };

        self.prediction_client
            .multi_inference(&request)
            .context("sending request to server")
            .map_err(From::from)
        */
    }

    /// Get model metadata
    pub fn get_model_metadata<S, T>(
        &self,
        model_name: S,
    ) -> Result<get_model_metadata::GetModelMetadataResponse>
    where
        T: Into<String>,
        S: Into<ModelDescription<T>>,
    {
        let request = get_model_metadata::GetModelMetadataRequest {
            model_spec: Some(self.build_model_spec(model_name)).into(),
            metadata_field: vec!["signature_def".to_string()].into(),
            ..Default::default()
        };

        self.prediction_client
            .get_model_metadata(&request)
            .context("sending request to server")
            .map_err(From::from)
    }

    /// Get model status
    pub fn get_model_status<S, T>(
        &self,
        model_name: S,
    ) -> Result<get_model_status::GetModelStatusResponse>
    where
        T: Into<String>,
        S: Into<ModelDescription<T>>,
    {
        let request = get_model_status::GetModelStatusRequest {
            model_spec: Some(self.build_model_spec(model_name)).into(),
            ..Default::default()
        };

        self.model_management_client
            .get_model_status(&request)
            .context("sending request to server")
            .map_err(From::from)
    }

    /// Reload the model configs
    pub fn reload_config<H>(&self, model_map: H) -> Result<model_management::ReloadConfigResponse>
    where
        H: Into<model_server_config::ModelConfigList>,
    {
        // request -> modelserverconfig -> ModelConfigList
        //
        let config_list = model_map.into();
        let model_server_config = model_server_config::ModelServerConfig {
            config: Some(
                model_server_config::ModelServerConfig_oneof_config::model_config_list(config_list),
            ),
            ..Default::default()
        };

        let request = model_management::ReloadConfigRequest {
            config: Some(model_server_config).into(),
            ..Default::default()
        };

        self.model_management_client
            .handle_reload_config_request(&request)
            .context("sending request to server")
            .map_err(From::from)
    }
    */

    // Private helper functions
    fn build_input<S, V>(&self, payload_map: HashMap<S, V>) -> Input
    where
        S: Into<String>,
        V: Into<Payload>,
    {
        // Build Feature
        let ft = payload_map.to_features();

        // Build Vec<Example>
        let example = Example {
            features: Some(ft).into(),
            ..Default::default()
        };
        // Build ExampleList
        let example_list = ExampleList {
            examples: vec![example].into(),
            ..Default::default()
        };
        // Build Input
        let input = Input {
            kind: Some(input::Kind::ExampleList(example_list)).into(),
            ..Default::default()
        };

        input
    }

    fn build_model_spec<S, T>(&self, model_description: S) -> ModelSpec
    where
        S: Into<ModelDescription<T>>,
        T: Into<String>,
    {
        let desc = model_description.into();

        let version = desc
            .version
            .map(|version_id| VersionChoice::Version(version_id));

        ModelSpec {
            name: desc.name.into(),
            version_choice: version,
            signature_name: self.signature_name.clone(),
            ..Default::default()
        }
    }
}

/// Result of prediction
#[derive(Debug)]
pub struct PredictionResult {
    /// Probability vector one per class
    ///
    pub probabilities: Vec<f32>,

    /// Index into the probability vector of the most likely class
    ///
    pub max_idx: i64,
}

impl PredictionResult {
    // fn from_raw(response: PredictResponse) -> Result<Self> {
    //     let outputs = response.get_outputs();
    //     let probs = outputs.get("probabilities").ok_or_else(|| {
    //         Err(format!("probabilities not available from the server response").into())
    //     })?;

    //     // Read the classes information
    //     let classes = outputs
    //         .get("classes")
    //         .ok_or_else(|| Err(format!("classes not available from the server response").into()))?;

    //     if classes.dtype != DataType::DtInt64 {
    //         return Err(format!(
    //             "classes has unexpected data type, should be i64, got {:?}",
    //             classes.dtype
    //         )
    //         .into());
    //     }

    //     let classes_shape = classes.get_tensor_shape();
    //     let dims = classes_shape.get_dim();

    //     if dims.len() != 1 {
    //         return Err(format!("number of classes unexpected").into());
    //     }

    //     let dim = &dims[0];
    //     let n = dim.size;
    //     if n != 1 {
    //         return Err(format!("number of classes unexpected").into());
    //     }

    //     let max_idx = classes.get_int64_val()[0];

    //     Ok(PredictionResult {
    //         probabilities: probs.float_val.clone(),
    //         max_idx,
    //     })
    // }
}
/// Description of a model
///
/// This struct is used to specify a model, and optionally a version of a model. It
/// implements From<Into<String>> so a `str` or `String` can be used to specify a model name
/// without worrying about a version number:
///
/// ```rust
/// # use tensorflow_serving::ModelDescription;
/// let model_name = "resnet";
/// let description: ModelDescription<_> = model_name.into();
/// ```
#[derive(Debug, Default)]
pub struct ModelDescription<S>
where
    S: Into<String>,
{
    /// Name of the model
    pub name: S,
    /// Optional version of the model
    pub version: Option<i64>,
}

impl<S> From<S> for ModelDescription<S>
where
    S: Into<String>,
{
    fn from(s: S) -> Self {
        ModelDescription {
            name: s,
            version: None,
        }
    }
}

/// Clssification payload
///
/// Can be either bytes, ints, or floats
pub enum Payload {
    /// Bytes payload
    Bytes(Vec<Vec<u8>>),
    /// Ints payload
    Ints(Vec<i64>),
    /// Floats payload
    Floats(Vec<f32>),
}

impl From<Vec<Vec<u8>>> for Payload {
    fn from(v: Vec<Vec<u8>>) -> Self {
        Payload::Bytes(v)
    }
}

impl From<Vec<i64>> for Payload {
    fn from(v: Vec<i64>) -> Self {
        Payload::Ints(v)
    }
}

impl From<Vec<f32>> for Payload {
    fn from(v: Vec<f32>) -> Self {
        Payload::Floats(v)
    }
}

impl From<Payload> for Feature {
    fn from(c: Payload) -> Self {
        let data_list = match c {
            Payload::Bytes(v) => {
                let data_list = BytesList {
                    value: v.into(),
                    ..Default::default()
                };

                Kind::BytesList(data_list)
            }
            Payload::Ints(v) => {
                let data_list = Int64List {
                    value: v.into(),
                    ..Default::default()
                };

                Kind::Int64List(data_list)
            }
            Payload::Floats(v) => {
                let data_list = FloatList {
                    value: v.into(),
                    ..Default::default()
                };

                Kind::FloatList(data_list)
            }
        };

        Feature {
            kind: Some(data_list),
            ..Default::default()
        }
    }
}

trait MapToFeatures {
    fn to_features(self) -> Features;
}

impl<S, V> MapToFeatures for HashMap<S, V>
where
    S: Into<String>,
    V: Into<Payload>,
{
    fn to_features(self) -> Features {
        let i = self.into_iter().map(|(k, v)| {
            let payload = v.into();
            let mut feature = Feature::default();

            match payload {
                Payload::Bytes(v) => {
                    feature.kind = Some(feature::Kind::BytesList(BytesList { value: v }));
                }
                Payload::Ints(v) => {
                    feature.kind = Some(feature::Kind::Int64List(Int64List { value: v }));
                }
                Payload::Floats(v) => {
                    feature.kind = Some(feature::Kind::FloatList(FloatList { value: v }));
                }
            }

            (k.into(), feature)
        });
        Features {
            feature: HashMap::from_iter(i),
            ..Default::default()
        }
    }
}