package com.niklashanft.echo.client.util;

import javax.sound.sampled.AudioFormat;

public class EchoAudioFormat {
    public static final AudioFormat ECHO_AUDIO_FORMAT = new AudioFormat(AudioFormat.Encoding.PCM_SIGNED, 48000, 16, 1, 2, 48000, false);
}
