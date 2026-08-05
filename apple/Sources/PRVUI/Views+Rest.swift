import Foundation
import PRVCore
import PRVKit

// The four spaces that used to draw a placeholder.
//
// # Authored, not verified
//
// As with `Views.swift`: SwiftUI is unavailable to the continuous integration
// this project has, so nothing below is compiled by any gate. That is why these
// views are as thin as they are. Every string is a key, every number arrives
// formatted, and every decision — which clip is next, what may interrupt a
// performance, what the summary sentence says — lives in a model that *is*
// compiled and tested on every commit.
//
// A view here can be wrong about a colour. It cannot be wrong about the product.

#if canImport(SwiftUI)
    import SwiftUI

    /// Where somebody lands: what exists, and the one thing to do next.
    public struct HomeSpace: View {
        private let model: HomeModel
        private let onGo: () -> Void
        private let onOpen: (String) -> Void

        public init(
            model: HomeModel,
            onGo: @escaping () -> Void,
            onOpen: @escaping (String) -> Void = { _ in }
        ) {
            self.model = model
            self.onGo = onGo
            self.onOpen = onOpen
        }

        public var body: some View {
            VStack(alignment: .leading, spacing: 16) {
                ForEach(model.standings) { standing in
                    HStack {
                        Text(LocalizedStringKey(standing.titleKey))
                        Spacer()
                        Text(standing.value)
                            .monospacedDigit()
                            .foregroundStyle(standing.needsAttention ? .primary : .secondary)
                        if standing.needsAttention {
                            Image(systemName: "exclamationmark.circle")
                        }
                    }
                }

                Divider()

                // One suggestion, not four. A screen offering several next steps
                // is a screen that has not decided.
                Button(action: onGo) {
                    Label(
                        LocalizedStringKey(model.nextStepKey),
                        systemImage: "arrow.forward.circle"
                    )
                }
                .buttonStyle(.borderedProminent)

                if !model.otherProjects.isEmpty {
                    Divider()
                    Text(LocalizedStringKey("home.projects"))
                        .font(.headline)
                    ForEach(model.otherProjects, id: \.self) { name in
                        Button(name) { onOpen(name) }
                            .buttonStyle(.link)
                    }
                }

                Spacer()
            }
            .padding()
        }
    }

    /// One clip, drawn as a block on a lane.
    public struct ClipView: View {
        private let clip: ClipModel
        private let isSelected: Bool
        private let onSelect: () -> Void

        public init(clip: ClipModel, isSelected: Bool, onSelect: @escaping () -> Void) {
            self.clip = clip
            self.isSelected = isSelected
            self.onSelect = onSelect
        }

        public var body: some View {
            Button(action: onSelect) {
                Text(clip.label)
                    .lineLimit(1)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(isSelected ? Color.accentColor.opacity(0.35) : Color.secondary.opacity(0.18))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Text(clip.label))
        }
    }

    /// The timeline, and the edits that can be made to it.
    public struct MixEditorSpace: View {
        private let model: MixEditorModel
        @Binding private var selection: UInt64?
        private let onRemove: (UInt64) -> Void
        private let onUndo: () -> Void

        public init(
            model: MixEditorModel,
            selection: Binding<UInt64?>,
            onRemove: @escaping (UInt64) -> Void,
            onUndo: @escaping () -> Void
        ) {
            self.model = model
            self._selection = selection
            self.onRemove = onRemove
            self.onUndo = onUndo
        }

        public var body: some View {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text(LocalizedStringKey("mix.length"))
                    Text(model.durationText).monospacedDigit()
                    Spacer()

                    Button(action: onUndo) {
                        Label(LocalizedStringKey("mix.undo"), systemImage: "arrow.uturn.backward")
                    }
                    .disabled(!model.canUndo)

                    if let blocked = model.undoBlockedKey {
                        // A sentence rather than a greyed control: "somebody
                        // else moved this since" is information, and hiding it
                        // behind a disabled button reads as a broken
                        // application.
                        Label(LocalizedStringKey(blocked), systemImage: "person.2")
                            .foregroundStyle(.secondary)
                    }
                }

                if model.isEmpty {
                    ContentUnavailableView(
                        LocalizedStringKey("mix.empty"),
                        systemImage: "slider.horizontal.3",
                        description: Text(LocalizedStringKey("mix.empty.detail"))
                    )
                } else {
                    List(selection: $selection) {
                        ForEach(model.inTimeOrder) { clip in
                            HStack {
                                ClipView(
                                    clip: clip,
                                    isSelected: selection == clip.id,
                                    onSelect: { selection = clip.id }
                                )
                                Spacer()
                                Text(LocalizedStringKey("mix.lane"))
                                    .foregroundStyle(.secondary)
                                Text("\(clip.placement.lane)").monospacedDigit()
                                Button(role: .destructive) {
                                    onRemove(clip.id)
                                } label: {
                                    Image(systemName: "trash")
                                }
                                .buttonStyle(.borderless)
                                .accessibilityLabel(Text(LocalizedStringKey("mix.remove")))
                            }
                            .tag(clip.id)
                        }
                    }
                }

                Text(LocalizedStringKey("mix.history"))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    + Text(" \(model.historyLength)").font(.footnote).monospacedDigit()
            }
            .padding()
        }
    }

    /// The performance view.
    public struct LiveSpace: View {
        private let model: LiveModel
        private let onPlay: () -> Void
        private let onPause: () -> Void

        public init(
            model: LiveModel,
            onPlay: @escaping () -> Void,
            onPause: @escaping () -> Void
        ) {
            self.model = model
            self.onPlay = onPlay
            self.onPause = onPause
        }

        public var body: some View {
            VStack(spacing: 20) {
                // The only thing permitted to interrupt: it concerns the sound
                // coming out right now. There is deliberately no queue here for
                // anything else to be added to.
                if let warning = model.warningKey {
                    Label(LocalizedStringKey(warning), systemImage: "exclamationmark.triangle")
                        .padding(8)
                        .frame(maxWidth: .infinity)
                        .background(Color.orange.opacity(0.2))
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                }

                VStack(spacing: 4) {
                    Text(LocalizedStringKey("live.now"))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(model.nowPlaying?.label ?? "—")
                        .font(.title2)
                        .lineLimit(1)
                    Text(model.positionText)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }

                if let next = model.upNext {
                    VStack(spacing: 4) {
                        Text(LocalizedStringKey("live.next"))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Text(next.label).lineLimit(1)
                        if let countdown = model.timeToNext {
                            Text(countdown).monospacedDigit().foregroundStyle(.secondary)
                        }
                    }
                }

                Spacer()
                TransportBar(model: model.transport, onPlay: onPlay, onPause: onPause)
            }
            .padding()
        }
    }

    /// Privacy, synchronisation and the licence — in that order.
    public struct SettingsSpace: View {
        private let model: SettingsModel
        private let onToggle: (Purpose, Bool) -> Void

        public init(model: SettingsModel, onToggle: @escaping (Purpose, Bool) -> Void) {
            self.model = model
            self.onToggle = onToggle
        }

        public var body: some View {
            Form {
                Section(LocalizedStringKey("settings.privacy")) {
                    Label(
                        LocalizedStringKey(model.consent.summaryKey),
                        systemImage: model.consent.anyOfTheirWorkLeaves
                            ? "arrow.up.circle" : "lock"
                    )
                    ForEach(model.consent.rows) { row in
                        Toggle(isOn: Binding(
                            get: { row.isGranted },
                            set: { onToggle(row.purpose, $0) }
                        )) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(LocalizedStringKey(row.titleKey))
                                Text(LocalizedStringKey(row.effectKey))
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }

                Section(LocalizedStringKey("settings.sync")) {
                    LabeledContent(LocalizedStringKey("settings.sync.state")) {
                        Text(LocalizedStringKey(model.sync.statusKey))
                    }
                    LabeledContent(LocalizedStringKey("settings.sync.waiting")) {
                        Text("\(model.sync.waiting)").monospacedDigit()
                    }
                    if let badge = model.sync.badgeKey {
                        Label(LocalizedStringKey(badge), systemImage: "exclamationmark.circle")
                    }
                }

                Section(LocalizedStringKey("settings.licence")) {
                    LabeledContent(LocalizedStringKey("settings.licence.tier")) {
                        Text(LocalizedStringKey(model.tierKey))
                    }
                }
            }
            .formStyle(.grouped)
        }
    }
#endif
