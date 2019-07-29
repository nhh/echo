package dev.nhh.echoclient.audio.speaker;

import javax.sound.sampled.AudioFormat;
import javax.sound.sampled.AudioSystem;
import javax.sound.sampled.DataLine.Info;
import javax.sound.sampled.LineUnavailableException;
import javax.sound.sampled.SourceDataLine;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.SocketException;
import java.util.concurrent.atomic.AtomicBoolean;

public enum Speaker {
    INSTANCE;

    private Thread thread;
    private SourceDataLine speakers;
    private final AudioFormat format = new AudioFormat(AudioFormat.Encoding.PCM_SIGNED, 44100, 16, 2, 4, 44100,false);
    private final Info info = new Info(SourceDataLine.class, format);
    private DatagramSocket socket;
    private AtomicBoolean isRunning = new AtomicBoolean(true);

    public void start(DatagramSocket socket) {
        if (this.isRunning.get()) {
            return;
        }

        this.socket = socket;
        this.isRunning.set(true);

        try {
            speakers = (SourceDataLine) AudioSystem.getLine(info);
            speakers.open(format);
            speakers.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }

        this.thread = new Thread(() -> {
            while(this.isRunning.get()) {
                final byte[] buffer = new byte[512];
                final var request = new DatagramPacket(buffer, 512);
                try {
                    this.socket.receive(request);
                    this.speakers.write(request.getData(), request.getOffset(), request.getLength());
                } catch(IOException e) {
                    e.printStackTrace();
                }
            }
            this.speakers.stop();
            this.speakers.flush();
            this.speakers.close();
        });
        this.thread.start();
    }

    public void stop() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
    }

}
