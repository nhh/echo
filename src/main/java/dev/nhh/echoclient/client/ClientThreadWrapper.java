package dev.nhh.echoclient.client;

import dev.nhh.echoclient.audio.speaker.AudioPacket;
import dev.nhh.echoclient.audio.Microphone;
import dev.nhh.echoclient.audio.speaker.SpeakerQueue;
import dev.nhh.echoclient.util.ThreadWrapper;

import java.io.IOException;
import java.net.*;
import java.util.logging.Level;
import java.util.logging.Logger;

public class ClientThreadWrapper extends ThreadWrapper {

    private final Logger logger = Logger.getLogger("ClientThread");
    private DatagramSocket socket;
    private InetAddress address;

    public ClientThreadWrapper(String hostname) {
        super("clientThreadWrapper", true);
        System.out.println("Connecting to " + hostname);
        try {
            address = InetAddress.getByName(hostname);
        } catch (UnknownHostException e) {
            e.printStackTrace();
        }
    }

    @Override
    public void run() {

        logger.log(Level.INFO, this.getName() + " is started!");

        try {
            socket = new DatagramSocket();
        } catch (SocketException e) {
            e.printStackTrace();
        }

        final var microphone = Microphone.INSTANCE;
        final var speakerQueue = SpeakerQueue.INSTANCE.getQueue();

        while(this.isRunning.get()) {
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

}
