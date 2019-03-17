package dev.nhh.echo.client;

import dev.nhh.echo.audio.speaker.AudioPacket;
import dev.nhh.echo.audio.Microphone;
import dev.nhh.echo.audio.speaker.SpeakerQueue;

import java.io.IOException;
import java.net.*;

public class Client implements Runnable {

    private DatagramSocket socket;
    private InetAddress address;
    private boolean running;

    public Client(String hostname) {
        System.out.println("Connecting to " + hostname);
        try {
            address = InetAddress.getByName(hostname);
        } catch (UnknownHostException e) {
            e.printStackTrace();
        }
    }

    public void run() {
        running = true;

        try {
            socket = new DatagramSocket();
        } catch (SocketException e) {
            e.printStackTrace();
        }

        final var microphone = Microphone.INSTANCE;
        final var speakerQueue = SpeakerQueue.INSTANCE.getQueue();

        while(running) {
            microphone.read(0, 1024);
            DatagramPacket packet = new DatagramPacket(microphone.getData(), microphone.getNumBytesRead(), address, 4445);

            final byte[] buffer = new byte[1024];
            final var response = new DatagramPacket(buffer, buffer.length);

            try {
                socket.send(packet);
                socket.receive(response);
            } catch(IOException e) {
                e.printStackTrace();
            }

            speakerQueue.add(new AudioPacket(response.getData(), 0, response.getData().length));
        }

    }

    public void close() {
        socket.close();
    }

}
