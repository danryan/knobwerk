use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use slint::ComponentHandle;
use ui::{AppWindow, Params};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let params: Arc<Params> = Arc::new(Params::new());

    let app = AppWindow::new()?;

    // Push the Slint defaults into the shared Params so the audio thread sees
    // the same values as the UI on the very first buffer.
    params.set_gain(app.get_gain());
    params.set_delay(app.get_delay());
    params.set_reverb(app.get_reverb());
    params.set_bypass(app.get_bypass());

    {
        let params = params.clone();
        app.on_param_changed(move |name, value| {
            params.apply_named(name.as_str(), value);
        });
    }
    {
        let params = params.clone();
        app.on_bypass_changed(move |b| {
            params.set_bypass(b);
        });
    }

    // Hold the cpal stream alive for the lifetime of `app.run()`.
    let _stream = match build_audio_stream(params.clone()) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("audio: failed to start output stream: {e}; UI will run silently");
            None
        }
    };

    app.run()?;
    Ok(())
}

fn build_audio_stream(params: Arc<Params>) -> Result<cpal::Stream, Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device.default_output_config()?;
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    if sample_format != cpal::SampleFormat::F32 {
        return Err(format!(
            "expected f32 output, got {sample_format:?}; this demo is f32-only"
        )
        .into());
    }

    let sample_rate = config.sample_rate.0 as f32;
    let channels = config.channels as usize;

    // 250 ms single-tap delay line at the device sample rate.
    let delay_len = (sample_rate * 0.25) as usize;
    let mut delay_buf: Vec<f32> = vec![0.0; delay_len.max(1)];
    let mut delay_idx: usize = 0;

    let mut phase: f32 = 0.0;
    let phase_inc = 2.0 * core::f32::consts::PI * 440.0 / sample_rate;

    let err_cb = |e| eprintln!("audio: stream error: {e}");

    let stream = device.build_output_stream::<f32, _, _>(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let bypass = params.bypass();
            let gain = params.gain();
            let delay_amt = params.delay();
            let reverb_amt = params.reverb();

            for frame in data.chunks_mut(channels) {
                let sample = if bypass {
                    0.0
                } else {
                    // 440 Hz sine source.
                    phase += phase_inc;
                    if phase > 2.0 * core::f32::consts::PI {
                        phase -= 2.0 * core::f32::consts::PI;
                    }
                    let dry = phase.sin() * gain;

                    // Single-tap delay line.
                    let delayed = delay_buf[delay_idx];
                    let delayed_scaled = delayed * delay_amt;
                    delay_buf[delay_idx] = dry + delayed_scaled * 0.5;
                    delay_idx = (delay_idx + 1) % delay_buf.len();

                    // Crude wet/dry mix as a stand-in for reverb.
                    dry * (1.0 - reverb_amt) + delayed_scaled * reverb_amt
                };

                for s in frame.iter_mut() {
                    *s = sample;
                }
            }
        },
        err_cb,
        None,
    )?;

    stream.play()?;
    Ok(stream)
}
