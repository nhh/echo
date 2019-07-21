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

public class Speaker implements Runnable {

    private SourceDataLine speakers;
    private final AudioFormat format = new AudioFormat(16000, 16, 1, true, true);
    private final Info info = new Info(SourceDataLine.class, format);
    private boolean isRunning = true;
    private DatagramSocket socket;

    public Speaker() {
        try {
            speakers = (SourceDataLine) AudioSystem.getLine(info);
            speakers.open(format);
            speakers.start();
        } catch (LineUnavailableException e) {
            e.printStackTrace();
        }
    }


    @Override
    public void run() {
        // Todo fixup closing thread

        try {
            socket = new DatagramSocket(44455);
            System.out.println("Listening on 44455");
        } catch(SocketException e) {
            e.printStackTrace();
        }

        while(isRunning) {

            final byte[] buffer = new byte[128];

            final var request = new DatagramPacket(buffer, buffer.length);

            try {
                socket.receive(request);
                this.speakers.write(request.getData(), request.getOffset(), request.getLength());
            } catch(IOException e) {
                e.printStackTrace();
            }

            if (Thread.interrupted()) {
                return;
            }

        }
    }
}
