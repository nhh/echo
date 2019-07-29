package dev.nhh.echoclient.audio.microphone;

import javax.sound.sampled.*;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.UnknownHostException;
import java.util.concurrent.atomic.AtomicBoolean;

public enum Microphone {
    INSTANCE;

    private TargetDataLine microphone;
    private byte[] data;
    private DatagramSocket socket;
    private Thread thread;
    private AtomicBoolean isRunning = new AtomicBoolean(true);
    private final AudioFormat format = new AudioFormat(AudioFormat.Encoding.PCM_SIGNED, 44100, 16, 2, 4, 44100,false);

    public void start(DatagramSocket targetSocket) {

        if (this.isRunning.get()) {
            return;
        }

        this.socket = targetSocket;
        this.isRunning.set(true);

        try {
            microphone = AudioSystem.getTargetDataLine(format);
            microphone.open(format);
            data = new byte[microphone.getBufferSize() / 5];
            microphone.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }

        final InetAddress address;

        try {
            address = InetAddress.getByName("116.203.73.169");
        } catch (UnknownHostException e) {
            e.printStackTrace();
            return;
        }



        this.thread = new Thread(() -> {
            var stream = new AudioInputStream(microphone);

            while(isRunning.get()) {

                try {
                    int numBytesRead= stream.read(data, 0, 512);
                    final var packet = new DatagramPacket(data, numBytesRead, address, 4445);
                    socket.send(packet);
                } catch (IOException e) {
                    e.printStackTrace();
                }

                if (Thread.interrupted()) {
                    return;
                }

            }

            this.microphone.stop();
            this.microphone.flush();
            this.microphone.close();
        });

        this.thread.start();

    }

    public void stop() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
    }


}
