#import "NativeEditorFocusHostComponentView.h"

#if __has_include("ReactNativeProseEditor-Swift.h")
#import "ReactNativeProseEditor-Swift.h"
#else
#error "ReactNativeProseEditor Swift compatibility header is unavailable; verify the pod module name and consumer codegen"
#endif

#include <react/renderer/components/ReactNativeProseEditorSpec/ComponentDescriptors.h>
#include <react/renderer/components/ReactNativeProseEditorSpec/Props.h>
#import <react/renderer/components/ReactNativeProseEditorSpec/RCTComponentViewHelpers.h>

using namespace facebook::react;

@interface NativeEditorFocusHostComponentView () <RCTNativeEditorFocusHostViewProtocol>
@end

@implementation NativeEditorFocusHostComponentView

+ (ComponentDescriptorProvider)componentDescriptorProvider
{
  return concreteComponentDescriptorProvider<NativeEditorFocusHostComponentDescriptor>();
}

- (instancetype)initWithFrame:(CGRect)frame
{
  if (self = [super initWithFrame:frame]) {
    static const auto defaultProps = std::make_shared<const NativeEditorFocusHostProps>();
    _props = defaultProps;
  }
  return self;
}

- (void)handleCommand:(const NSString *)commandName args:(const NSArray *)args
{
  RCTNativeEditorFocusHostHandleCommand(self, commandName, args);
}

- (void)blur
{
  [NativeEditorFocusHost blurEditorBesideHost:self];
}

@end
