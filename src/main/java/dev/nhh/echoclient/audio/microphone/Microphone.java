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

    public Microphone() {
        try {
            microphone = AudioSystem.getTargetDataLine(format);
            microphone.open(format);
            data = new byte[microphone.getBufferSize() / 5];
            microphone.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }

    private final AudioFormat format = new AudioFormat(
            16000,
            16,
            1,
            true,
            true
    );

    public void drain() {
        microphone.drain();
    }

    @Override
    public void run() {

        System.out.println("Running microphone");

        final InetAddress address;

        try {
            address = InetAddress.getByName("localhost");
        } catch (UnknownHostException e) {
            e.printStackTrace();
            return;
        }

        try {
            socket = new DatagramSocket();
            // Todo maybe remove new variable instantiation?


        } catch (IOException e) {
            e.printStackTrace();
        }

        boolean isRunning = true;

        while(isRunning) {

            int numBytesRead = microphone.read(data, 0, 128);
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
