package dev.nhh.echo.client.util;

import java.util.concurrent.ConcurrentLinkedQueue;

public enum ConnectionList {
    INSTANCE;

    private final ConcurrentLinkedQueue<Connection> connections = new ConcurrentLinkedQueue<>();

    public void addConnection(Connection connection) {
        if(this.connections.contains(connection)) {
            return;
        }
        this.connections.add(connection);
    }

    public void stopAllConnections() {
        System.out.println("Stopping all connections...");
        this.connections.forEach(Connection::shutdown);
        System.out.println("Done...");
    }

}
