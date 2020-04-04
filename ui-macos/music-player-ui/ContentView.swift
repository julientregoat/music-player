//
//  ContentView.swift
//  music-player-ui
//
//  Created by Julien Tregoat on 4/4/20.
//  Copyright © 2020 JULES.NYC. All rights reserved.
//

import SwiftUI

struct TrackData: Identifiable {
    var id: UInt64
    var name: String
    var artist: String
    var album: String
    var genre: String
    var dateAdded: String
    var duration: UInt64
    var bitRate: UInt8
    var sampleRate: UInt64
}

var tracks = [
    TrackData(
        id: 0,
        name: "Sandstorm",
        artist: "Darude",
        album: "Who knows",
        genre: "Shit",
        dateAdded: "Never",
        duration: 666,
        bitRate: 24,
        sampleRate: 48000
    )
]

struct TrackListing: View {
    let data: TrackData
    
    var body: some View {
        HStack {
            Group {
                Text(data.name)
                Spacer()
                Text(String(data.duration))
                Spacer()
                Text(data.artist)
                Spacer()
                Text(data.album)
                Spacer()
                Text(data.genre)
                Spacer()
            }
            Group {
                Text(data.dateAdded)
                Spacer()
                Text(String(data.bitRate))
                Spacer()
                Text(String(data.sampleRate))
            }
        }
    }
}
func parse_pointer(data: StringData) -> String {
    print("text addr", data.start.debugDescription)
    let str_buf = UnsafeBufferPointer(start: data.start, count: Int(data.len))
    return String(data: Data(str_buf), encoding: .utf8) ?? "failed"
}

struct MenuBar: View {
    let rust_val = parse_pointer(data: get_str())
    var body: some View {
        HStack {
            Text("librarian")
                .font(.title)
                .bold()
                .italic()
                .foregroundColor(.white)
                .multilineTextAlignment(.leading)
            MenuButton(label: Text("Menu")) {
                Text("Import")
                Text("New Playlist")
                Text(rust_val)
            }
            .frame(maxWidth: 400)
        }
    }
}

struct ContentView: View {
    var body: some View {
        VStack {
            MenuBar()

            List {
                HStack {
                    // TODO use iterator
                    Group {
                        Text("Track Name")
                        Spacer()
                        Text("Duration")
                        Spacer()
                        Text("Artist")
                        Spacer()
                        Text("Album")
                        Spacer()
                        Text("Genre")
                        Spacer()
                    }
                    Group {
                        Text("Date Added")
                        Spacer()
                        Text("Bit Rate")
                        Spacer()
                        Text("Sample Rate")
                    }
                }
                .padding(2)
                .border(Color.white)
                .shadow(radius: 1)
                ForEach(tracks) { t in TrackListing(data: t)}
            }
            .background(Color.gray)
        }
    }
    
}


struct ContentView_Previews: PreviewProvider {
    static var previews: some View {
        ContentView()
    }
}
