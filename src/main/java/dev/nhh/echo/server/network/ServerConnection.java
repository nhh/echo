package dev.nhh.echo.server.network;

import dev.nhh.echo.server.model.Channel;

import java.io.IOException;
import java.net.ServerSocket;
import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class ServerConnection extends Thread {

    private AtomicBoolean isRunning = new AtomicBoolean(false);
    private ArrayList<Channel> channels = new ArrayList<>();
    private ServerSocket socket;

    public ServerConnection(int port) {
        try {
            this.socket = new ServerSocket(port);
        } catch (IOException e) {
            e.printStackTrace();
        }

        var channel = new Channel(UUID.fromString("cc5a988c-236b-44b9-85fa-99b24290bf03"));
        channel.start(); // Todo kill me for not starting threads :O
        this.channels.add(channel);
    }

    @Override
    public void run() {

        if(this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while (isRunning.get()) {
            try {
                var clientConnection = socket.accept();
                System.out.println("Accepting client" + clientConnection.getInetAddress());
                var connection = new ClientConnection(clientConnection);
                connection.start();
                this.channels.get(0).addClient(connection);
            } catch (IOException e) {
                e.printStackTrace();
            }
        }

    }

}

//
