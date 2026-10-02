// pavo-vision: cuts the subject out of a photo with Apple's Vision framework, the same on-device
// model as "Lift subject from background" in Photos. The rust engine runs it like any other tool.
//
//   pavo-vision remove-background <photo> <out.png>   subject on transparency
//   pavo-vision white-background  <photo> <out.jpg>   subject on white
//   pavo-vision find-text         <image>             where text sits: one json line per box,
//                                                       x/y/w/h as fractions of the image, from the top left
import CoreImage
import Foundation
import Vision

enum Failure: Error, CustomStringConvertible {
    case usage
    case unreadable(String)
    case noSubject(String)
    case unwritable(String)

    var description: String {
        switch self {
        case .usage: "usage: pavo-vision remove-background|white-background <photo> <output>, or find-text <image>"
        case .unreadable(let name): "couldn't read \(name)"
        case .noSubject(let name): "couldn't find a subject in \(name)"
        case .unwritable(let name): "couldn't save \(name)"
        }
    }
}

/// The photo with everything but its subject made transparent, upright and at full size.
func cutOut(_ input: URL) throws(Failure) -> CIImage {
    let name = input.lastPathComponent
    guard let photo = CIImage(contentsOf: input, options: [.applyOrientationProperty: true]) else {
        throw .unreadable(name)
    }
    let handler = VNImageRequestHandler(ciImage: photo)
    let request = VNGenerateForegroundInstanceMaskRequest()
    do {
        try handler.perform([request])
        guard let subject = request.results?.first, !subject.allInstances.isEmpty else { throw Failure.noSubject(name) }
        let masked = try subject.generateMaskedImage(ofInstances: subject.allInstances, from: handler, croppedToInstancesExtent: false)
        return CIImage(cvPixelBuffer: masked)
    } catch let failure as Failure {
        throw failure
    } catch {
        throw .noSubject(name)
    }
}

/// Every piece of text Vision can read in the image, with where it sits.
func findText(_ input: URL) throws(Failure) {
    let name = input.lastPathComponent
    guard let image = CIImage(contentsOf: input) else { throw .unreadable(name) }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .fast // positions matter here, not perfect spelling
    do {
        try VNImageRequestHandler(ciImage: image).perform([request])
    } catch {
        throw .unreadable(name)
    }
    for observation in request.results ?? [] {
        guard let text = observation.topCandidates(1).first?.string else { continue }
        let box = observation.boundingBox // vision measures from the bottom left
        let line: [String: Any] = ["text": text, "x": box.minX, "y": 1 - box.maxY, "w": box.width, "h": box.height]
        if let json = try? JSONSerialization.data(withJSONObject: line) {
            FileHandle.standardOutput.write(json + Data("\n".utf8))
        }
    }
}

func run(_ arguments: [String]) throws(Failure) {
    if arguments.count == 3, arguments[1] == "find-text" {
        return try findText(URL(fileURLWithPath: arguments[2]))
    }
    guard arguments.count == 4 else { throw .usage }
    let (mode, input, output) = (arguments[1], URL(fileURLWithPath: arguments[2]), URL(fileURLWithPath: arguments[3]))
    let subject = try cutOut(input)
    let context = CIContext()
    let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

    do {
        switch mode {
        case "remove-background":
            try context.writePNGRepresentation(of: subject, to: output, format: .RGBA8, colorSpace: sRGB)
        case "white-background":
            let white = CIImage(color: .white).cropped(to: subject.extent)
            try context.writeJPEGRepresentation(of: subject.composited(over: white), to: output, colorSpace: sRGB)
        default:
            throw Failure.usage
        }
    } catch let failure as Failure {
        throw failure
    } catch {
        throw .unwritable(output.lastPathComponent)
    }
}

do {
    try run(CommandLine.arguments)
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
