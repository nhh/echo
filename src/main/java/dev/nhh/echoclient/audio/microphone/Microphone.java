package dev.nhh.echoclient.audio.microphone;

import javax.sound.sampled.AudioFormat;
import javax.sound.sampled.AudioSystem;
import javax.sound.sampled.LineUnavailableException;
import javax.sound.sampled.TargetDataLine;
import java.io.IOException;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetAddress;
import java.net.UnknownHostException;

public class Microphone implements Runnable {

    private TargetDataLine microphone;
    private byte[] data;
    private DatagramSocket socket;

    private final AudioFormat format = new AudioFormat(44000, 16, 1, true,true);

    public Microphone(DatagramSocket socket) {
        this.socket = socket;
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

    @Override
    public void run() {

        boolean isRunning = true;

        final InetAddress address;

        try {
            address = InetAddress.getByName("116.203.73.169");
        } catch (UnknownHostException e) {
            e.printStackTrace();
            return;
        }

        while(isRunning) {

            int numBytesRead = microphone.read(data, 0, 512);

            final var packet = new DatagramPacket(data, numBytesRead, address, 4445);

            try {
                socket.send(packet);
            } catch (IOException e) {
                e.printStackTrace();
            }

            if (Thread.interrupted()) {
                return;
            }

        }

    }
}
