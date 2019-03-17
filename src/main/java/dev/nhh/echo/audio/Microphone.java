package dev.nhh.echo.audio;

import javax.sound.sampled.AudioFormat;
import javax.sound.sampled.AudioSystem;
import javax.sound.sampled.LineUnavailableException;
import javax.sound.sampled.TargetDataLine;

public enum Microphone {

    INSTANCE;

    private TargetDataLine microphone;
    private byte[] data;
    private int numBytesRead;

    private final AudioFormat format = new AudioFormat(
            16000,
            16,
            1,
            true,
            true
    );

    Microphone() {
        try {
            microphone = AudioSystem.getTargetDataLine(format);
            microphone.open(format);
            data = new byte[microphone.getBufferSize() / 5];
            microphone.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    public void drain() {
        microphone.drain();
    }

    public void read(int off, int length) {
        numBytesRead = microphone.read(data, off, length);
    }

    public byte[] getData() {
        return data;
    }

    public int getNumBytesRead() {
        return numBytesRead;
    }
}
