package dev.nhh.echo.client.controller;

import javafx.collections.FXCollections;
import javafx.collections.ObservableList;
import javafx.fxml.FXML;
import javafx.fxml.Initializable;
import javafx.scene.control.TreeItem;
import javafx.scene.control.TreeView;
import javafx.scene.image.Image;
import javafx.scene.image.ImageView;

import java.net.URL;
import java.util.ArrayList;
import java.util.ResourceBundle;

public class ChannelController implements Initializable {

    @FXML
    TreeView server;

    @Override
    public void initialize(URL location, ResourceBundle resources) {
        ImageView rootIcon = new ImageView(new Image(getClass().getResourceAsStream("/channel/1f4c1.png")));
        TreeItem<String> root = new TreeItem<>("German Rush Company", rootIcon);
        root.setExpanded(true);

        root.getChildren().addAll(this.getChannels());

        server.setRoot(root);
    }

    private ImageView getNewChannelIcon() {
        return new ImageView(new Image(getClass().getResourceAsStream("/channel/1f4c2.png")));
    }

    private ImageView getNewUserIcon() {
        return new ImageView(new Image(getClass().getResourceAsStream("/channel/1f518.png")));
    }

    private ObservableList<TreeItem<String>> getUsers() {
        final ArrayList<TreeItem<String>> users = new ArrayList<TreeItem<String>>();
        users.add(new TreeItem<>("ParadoXxGER @ Niklas", getNewUserIcon()));
        return FXCollections.observableList(users);
    }

    private ObservableList<TreeItem<String>> getChannels() {
        final ArrayList<TreeItem<String>> channels = new ArrayList<TreeItem<String>>();

        TreeItem channel = new TreeItem<>("Eingangshalle", getNewChannelIcon());
        channel.getChildren().addAll(getUsers());

        TreeItem channelOne = new TreeItem<>("Laberecke #1", getNewChannelIcon());
        channel.getChildren().addAll(getUsers());

        channels.add(channel);
        channels.add(channelOne);

        return FXCollections.observableList(channels);
    }


}
