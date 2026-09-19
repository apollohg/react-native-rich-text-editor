#import <XCTest/XCTest.h>

#import "../Viewer/Fabric/PREPPreparedProseViewerComponentView.h"
#import <react/renderer/components/ReactNativeProseEditorSpec/Props.h>

using namespace facebook::react;

@interface PreparedProseViewerComponentViewTests : XCTestCase
@end

@implementation PreparedProseViewerComponentViewTests

- (void)testPropUpdatesOnFreshAndRecycledViewer
{
    PREPPreparedProseViewerComponentView *view =
        [[PREPPreparedProseViewerComponentView alloc] initWithFrame:CGRectZero];
    // Prebuilt React Native binaries can compile out the constructor assertion.
    XCTAssertTrue(dynamic_cast<const PreparedProseViewerProps *>([view props].get()) != nullptr);
    auto firstProps = std::make_shared<PreparedProseViewerProps>();
    firstProps->opacity = 0.5;
    XCTAssertNoThrow([view updateProps:firstProps oldProps:nullptr]);
    XCTAssertEqualWithAccuracy(view.layer.opacity, 0.5, 0.001);

    auto nextProps = std::make_shared<PreparedProseViewerProps>();
    nextProps->opacity = 0.75;
    XCTAssertNoThrow([view updateProps:nextProps oldProps:firstProps]);
    XCTAssertEqualWithAccuracy(view.layer.opacity, 0.75, 0.001);

    [view prepareForRecycle];
    auto recycledProps = std::make_shared<PreparedProseViewerProps>();
    XCTAssertNoThrow([view updateProps:recycledProps oldProps:nullptr]);
    XCTAssertEqualWithAccuracy(view.layer.opacity, 1.0, 0.001);
}

@end
