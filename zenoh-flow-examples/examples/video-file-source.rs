//
// Copyright (c) 2017, 2021 ADLINK Technology Inc.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// http://www.eclipse.org/legal/epl-2.0, or the Apache License, Version 2.0
// which is available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//
// Contributors:
//   ADLINK zenoh team, <zenoh@adlink-labs.tech>
//

use async_std::sync::{Arc, Mutex};
use async_trait::async_trait;
use opencv::{core, prelude::*, videoio};
use std::collections::HashMap;
use zenoh_flow::{
    default_output_rule, downcast,
    types::{ZFError, ZFResult},
    zenoh_flow_derive::ZFState,
    zf_data_raw, zf_spin_lock, Component, OutputRule, SerDeData, Source, State,
};

#[derive(Debug)]
struct VideoSource;

#[derive(ZFState, Clone)]
struct VideoSourceState {
    pub camera: Arc<Mutex<videoio::VideoCapture>>,
    pub encode_options: Arc<Mutex<opencv::types::VectorOfi32>>,
    pub delay: u64,
    pub source_file: String,
}

// because of opencv
impl std::fmt::Debug for VideoSourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "VideoState: file:{:?} delay:{:?}",
            self.source_file, self.delay
        )
    }
}

static SOURCE: &str = "Frame";

impl VideoSourceState {
    fn new(configuration: &Option<HashMap<String, String>>) -> Self {
        // Configuration is mandatory
        let configuration = configuration.as_ref().unwrap();

        let source_file = configuration.get("file").unwrap();
        let delay = match configuration.get("fps") {
            Some(fps) => {
                let fps = fps.parse::<f64>().unwrap();
                let delay: f64 = 1f64 / fps;
                (delay * 1000f64) as u64
            }
            None => 40,
        };

        let camera = videoio::VideoCapture::from_file(source_file, videoio::CAP_ANY).unwrap(); // 0 is the default camera
        let opened = videoio::VideoCapture::is_opened(&camera).unwrap();
        if !opened {
            panic!("Unable to open default camera!");
        }
        Self {
            camera: Arc::new(Mutex::new(camera)),
            encode_options: Arc::new(Mutex::new(opencv::types::VectorOfi32::new())),
            source_file: source_file.to_string(),
            delay,
        }
    }
}

impl Component for VideoSource {
    fn initialize(
        &self,
        configuration: &Option<HashMap<String, String>>,
    ) -> Box<dyn zenoh_flow::State> {
        Box::new(VideoSourceState::new(configuration))
    }

    fn clean(&self, _state: &mut Box<dyn State>) -> ZFResult<()> {
        Ok(())
    }
}

impl OutputRule for VideoSource {
    fn output_rule(
        &self,
        _context: &mut zenoh_flow::Context,
        state: &mut Box<dyn zenoh_flow::State>,
        outputs: HashMap<zenoh_flow::PortId, SerDeData>,
    ) -> ZFResult<HashMap<zenoh_flow::PortId, zenoh_flow::ComponentOutput>> {
        default_output_rule(state, outputs)
    }
}

#[async_trait]
impl Source for VideoSource {
    async fn run(
        &self,
        _context: &mut zenoh_flow::Context,
        dyn_state: &mut Box<dyn zenoh_flow::State>,
    ) -> ZFResult<HashMap<zenoh_flow::PortId, SerDeData>> {
        let mut results: HashMap<zenoh_flow::PortId, SerDeData> = HashMap::new();

        let state = downcast!(VideoSourceState, dyn_state).unwrap();

        let mut cam = zf_spin_lock!(state.camera);
        let encode_options = zf_spin_lock!(state.encode_options);

        let mut frame = core::Mat::default();
        match cam.read(&mut frame) {
            Ok(false) => {
                *cam =
                    videoio::VideoCapture::from_file(&state.source_file, videoio::CAP_ANY).unwrap(); // 0 is the default camera
                let opened = videoio::VideoCapture::is_opened(&cam).unwrap();
                if !opened {
                    panic!("Unable to open default camera!");
                }
                cam.read(&mut frame)
                    .map_err(|e| ZFError::IOError(format!("{}", e)))?;
            }
            Ok(true) => (),
            Err(_) => {
                *cam =
                    videoio::VideoCapture::from_file(&state.source_file, videoio::CAP_ANY).unwrap(); // 0 is the default camera
                let opened = videoio::VideoCapture::is_opened(&cam).unwrap();
                if !opened {
                    panic!("Unable to open default camera!");
                }
                cam.read(&mut frame)
                    .map_err(|e| ZFError::IOError(format!("{}", e)))?;
            }
        };

        let mut buf = opencv::types::VectorOfu8::new();
        opencv::imgcodecs::imencode(".jpg", &frame, &mut buf, &encode_options).unwrap();

        results.insert(SOURCE.into(), zf_data_raw!(buf.into()));

        drop(cam);
        drop(encode_options);
        drop(frame);

        async_std::task::sleep(std::time::Duration::from_millis(state.delay)).await;

        Ok(results)
    }
}

zenoh_flow::export_source!(register);

fn register() -> ZFResult<Arc<dyn Source>> {
    Ok(Arc::new(VideoSource) as Arc<dyn Source>)
}
