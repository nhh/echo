package dev.nhh.echo.client.util;

import dev.nhh.echo.client.Client;
import dev.nhh.echo.client.audio.microphone.Microphone;
import dev.nhh.echo.client.audio.speaker.AudioChannel;
import dev.nhh.echo.client.dto.HandshakePacket;
import dev.nhh.echo.client.dto.VoicePacket;

import java.io.IOException;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.net.Socket;
import java.util.ArrayList;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.logging.Level;
import java.util.logging.Logger;

public class Connection extends Thread {

    private Socket socket;
    private final Logger logger = Logger.getLogger(Connection.class.getName());
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    private Microphone microphone;
    private final ArrayList<AudioChannel> audioChannels = new ArrayList<>();

    public Connection(String ip) {
        try {
            this.socket = new Socket(ip, 4445);
        } catch (IOException e) {
            e.printStackTrace();
        }
        this.logger.log(Level.INFO, "Connecting to " + ip);
    }

    @Override
    public void run() {

        if(this.isRunning.get()){
            return;
        }

        this.isRunning.set(true);

        ObjectInputStream fromServer;

        try {
            var stream = new ObjectOutputStream(socket.getOutputStream());
            stream.writeObject(new HandshakePacket(Client.CLIENT_ID, System.nanoTime() / 1000000L));
            System.out.println("Handshake done!");
            this.microphone = new Microphone(stream);
            this.microphone.start();
            fromServer = new ObjectInputStream(this.socket.getInputStream());
        } catch (IOException e) {
            e.printStackTrace();
            return;
        }

        while(this.isRunning.get()) {
            try {
                if(fromServer.available() == 0) {
                    try { Thread.sleep(10); } catch (InterruptedException e) { e.printStackTrace(); }
                }

                VoicePacket in = (VoicePacket) (fromServer.readObject());

                AudioChannel sendTo = null;

                for (AudioChannel channel : this.audioChannels) {
                    if (channel.getChannelId() == in.getChannelId()) {
                        sendTo = channel;
                    }
                }

                if(sendTo != null) {
                    sendTo.addVoicePacketToQueue(in);
                    continue;
                }

                var newChannel = new AudioChannel(in.getChannelId());
                newChannel.addVoicePacketToQueue(in);
                newChannel.start();
                this.audioChannels.add(newChannel);
            } catch (IOException | ClassNotFoundException e) {
                this.isRunning.set(false);
                System.out.println("Lost connection");
            }
        }

        // Close microphone and all AudioChannels

        this.microphone.shutdown();
        this.audioChannels.forEach(AudioChannel::shutdown);

        try {
            this.socket.close();
        } catch (IOException e) {
            e.printStackTrace();
        }

    }

    public void shutdown() {
        if(!this.isRunning.get()) return;
        this.isRunning.set(false);
    }

}
